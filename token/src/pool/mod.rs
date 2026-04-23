use near_sdk::json_types::U128;
use near_sdk::serde_json::json;
use near_sdk::{AccountId, Gas, NearToken, PublicKey, env, near, require};
use num_traits::ToPrimitive;

use crate::{LiquidStakingToken, LiquidStakingTokenExt};

pub use stake::StakeMessage;
pub use stats::PoolStatistics;
pub use unstake::{UnstakeMessage, WithdrawTokens};
pub use withdraw::UserDistribution;

mod admin;
mod stake;
mod stats;
mod unstake;
mod withdraw;

const FT_TRANSFER_GAS: Gas = Gas::from_tgas(2);
const FT_TRANSFER_CALL_GAS_MIN: Gas = Gas::from_tgas(30);
const MODIFY_STAKED_AMOUNT_GAS: Gas = Gas::from_tgas(2);
const STORAGE_DEPOSIT_GAS: Gas = Gas::from_tgas(2);
const MAX_RESULT_LENGTH: usize = "\"+340282366920938463463374607431768211455\"".len(); // u128::MAX

type LstToken = NearToken;

#[near]
impl LiquidStakingToken {
    /// Returns the number of delegators in the pool.
    pub const fn get_number_of_accounts(&self) -> u64 {
        self.statistics.num_delegators
    }

    /// Returns the current exchange rate of LST to NEAR.
    pub fn get_exchange_rate(&self) -> near_sdk::serde_json::Value {
        let total_staked = self.statistics.total_staked_amount.as_yoctonear();
        let total_lst_supply = self.token.total_supply;

        let (numerator, denominator) = if total_lst_supply == 0 {
            (1, 1)
        } else {
            (total_staked, total_lst_supply)
        };

        json!({
            "numerator": U128(numerator),
            "denominator": U128(denominator),
        })
    }

    pub fn get_reward_fee_fraction(&self) -> near_sdk::serde_json::Value {
        json!({
            "numerator": U128(self.statistics.withdrawal_fee_bps.into()),
            "denominator": U128(stats::BPS_DENOMINATOR.into()),
        })
    }

    /// Returns the public key of the validator that is currently staking.
    pub fn get_staking_key(&self) -> PublicKey {
        self.validator_public_key.clone()
    }

    /// Returns the account ID of the contract owner.
    pub fn get_owner_id(&self) -> AccountId {
        self.owner_id.clone()
    }

    /// Returns the total NEAR currently backing the LST supply. Grows as
    /// staking rewards are synced.
    pub const fn get_total_staked_balance(&self) -> NearToken {
        self.statistics.total_staked_amount
    }

    /// Returns the sum of NEAR amounts queued for withdrawal (pre- and
    /// post-cooldown).
    pub const fn get_total_pending_unstake(&self) -> NearToken {
        self.statistics.total_pending_unstake
    }

    /// Returns the current withdrawal fee in basis points (1 bp = 0.01%).
    pub const fn get_withdrawal_fee_bps(&self) -> u16 {
        self.statistics.withdrawal_fee_bps
    }

    /// Returns the NEAR amount accumulated from withdrawal fees, awaiting claim.
    pub const fn get_collected_fees(&self) -> NearToken {
        self.statistics.withdrawal_collected_fees
    }

    #[private]
    pub fn modify_total_staked_amount(
        &mut self,
        account_id: &AccountId,
        total_staked_tokens: NearToken,
        lst_tokens: LstToken,
        is_stake: bool,
    ) {
        self.statistics.total_staked_amount = total_staked_tokens;

        if is_stake {
            if self.is_zero_balance(account_id) {
                self.statistics.increase_delegators();
            }

            self.token
                .internal_deposit(account_id, lst_tokens.as_yoctonear());
        } else {
            self.token
                .internal_withdraw(account_id, lst_tokens.as_yoctonear());

            if self.is_zero_balance(account_id) {
                self.statistics.decrease_delegators();
            }
        }
    }

    /// Publicly callable rewards sync. Reads the contract's `locked` balance
    /// and, when it exceeds the tracked active stake plus pending unstakes,
    /// treats the excess as newly accrued validator rewards that get added to
    /// the LST's backing NEAR (lifting the exchange rate).
    ///
    /// Returns the NEAR amount of rewards recognized by this call.
    pub fn ping(&mut self) -> NearToken {
        self.sync_rewards_internal()
    }
}

impl LiquidStakingToken {
    /// Implementation of [`Self::sync_rewards`] usable from internal call sites.
    pub(crate) fn sync_rewards_internal(&mut self) -> NearToken {
        let current_epoch = env::epoch_height();

        if current_epoch == self.statistics.last_epoch_synced {
            return NearToken::ZERO;
        }

        self.statistics.last_epoch_synced = current_epoch;

        let actual_locked_balance = env::account_locked_balance();
        let expected_locked_balance = self
            .statistics
            .total_staked_amount
            .saturating_add(self.statistics.total_pending_unstake);

        if actual_locked_balance <= expected_locked_balance {
            return NearToken::ZERO;
        }

        let rewards = actual_locked_balance.saturating_sub(expected_locked_balance);

        self.statistics.total_staked_amount = self
            .statistics
            .total_staked_amount
            .checked_add(rewards)
            .unwrap_or_else(|| env::panic_str("Overflow while adding rewards"));

        near_sdk::log!(
            "Rewards synced: +{} yoctoNEAR (new total staked amount: {})",
            rewards.as_yoctonear(),
            self.statistics.total_staked_amount.as_yoctonear(),
        );

        rewards
    }

    /// Converts a NEAR amount into the LST mint amount at the current exchange
    /// rate. Uses a 1:1 ratio while no LST has been minted yet.
    pub(crate) fn near_to_lst(&self, near_tokens: NearToken) -> LstToken {
        let total_shared = self.token.total_supply;
        let total_staked = self.statistics.total_staked_amount.as_yoctonear();
        let yocto_amount = near_tokens.as_yoctonear();

        let yocto = if total_shared == 0 || total_staked == 0 {
            yocto_amount
        } else {
            mul_div_floor(yocto_amount, total_shared, total_staked)
        };

        LstToken::from_yoctonear(yocto)
    }

    /// Converts an LST amount into its NEAR equivalent at the current exchange
    /// rate. Returns zero when no LST has been minted yet.
    pub(crate) fn lst_to_near(&self, lst_tokens: LstToken) -> NearToken {
        let total_shared = self.token.total_supply;
        let total_staked = self.statistics.total_staked_amount.as_yoctonear();
        let yocto_amount = lst_tokens.as_yoctonear();

        if total_shared == 0 {
            return NearToken::ZERO;
        }

        NearToken::from_yoctonear(mul_div_floor(yocto_amount, total_staked, total_shared))
    }

    /// Applies the configured withdrawal fee. Returns `(amount_net, fee)`.
    pub(crate) fn split_withdrawal_fee(&self, amount: NearToken) -> (NearToken, NearToken) {
        if self.statistics.withdrawal_fee_bps == 0 {
            return (amount, NearToken::ZERO);
        }

        let fee_yocto = mul_div_floor(
            amount.as_yoctonear(),
            u128::from(self.statistics.withdrawal_fee_bps),
            u128::from(stats::BPS_DENOMINATOR),
        );

        let fee = NearToken::from_yoctonear(fee_yocto);
        let net = amount
            .checked_sub(fee)
            .unwrap_or_else(|| env::panic_str("Withdrawal fee exceeds the withdrawal amount"));

        (net, fee)
    }
}

#[inline]
fn calculate_min_gas(min_gas: Option<Gas>, is_call: bool) -> Gas {
    let min = if is_call {
        FT_TRANSFER_CALL_GAS_MIN
    } else {
        FT_TRANSFER_GAS
    };

    min_gas.unwrap_or(min).max(min)
}

#[inline]
#[must_use]
pub(crate) fn mul_div_floor(a: u128, b: u128, c: u128) -> u128 {
    require!(c > 0, "Division by zero in mul_div_floor");

    if let Some(product) = a.checked_mul(b) {
        return product / c;
    }

    let a_u256: ruint::aliases::U256 = ruint::Uint::from(a);
    let b_u256: ruint::aliases::U256 = ruint::Uint::from(b);
    let c_u256: ruint::aliases::U256 = ruint::Uint::from(c);

    a_u256
        .checked_mul(b_u256)
        .and_then(|prod| prod.checked_div(c_u256))
        .and_then(|result| result.to_u128())
        .unwrap_or_else(|| env::panic_str("Overflow in mul_div_floor"))
}

#[cfg(test)]
mod tests {
    use super::mul_div_floor;

    #[test]
    fn basic() {
        // 5 * 5 / 4 = 6.25, but we want to round down to 6
        assert_eq!(mul_div_floor(5, 5, 4), 6);
        // 9 * 3 / 4 = 6.75, but we want to round down to 6
        assert_eq!(mul_div_floor(9, 3, 4), 6);
        assert_eq!(mul_div_floor(0, 123, 7), 0);
        assert_eq!(mul_div_floor(7, 0, 3), 0);
    }

    #[test]
    fn basic_with_u256() {
        assert_eq!(mul_div_floor(u128::MAX, u128::MAX, u128::MAX), u128::MAX);
    }

    #[test]
    fn reward_bearing_growth() {
        // Supply 100, staked 110 (10% rewards). Staking 10 NEAR should mint ~9.09 LST.
        let mint = mul_div_floor(10, 100, 110);
        assert_eq!(mint, 9);
    }

    #[test]
    #[should_panic(expected = "Division by zero")]
    fn div_by_zero() {
        let _ = mul_div_floor(1, 1, 0);
    }

    #[test]
    #[should_panic(expected = "Overflow in mul_div_floor")]
    fn quotient_overflow() {
        let _ = mul_div_floor(u128::MAX, u128::MAX, 1);
    }
}
