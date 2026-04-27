use near_plugins::{Pausable, pause};
use near_sdk::json_types::U128;
use near_sdk::{AccountId, Gas, NearToken, Promise, PromiseOrValue, PublicKey, env, near, require};
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
const MODIFY_STATE_AFTER_STAKE_GAS: Gas = Gas::from_tgas(2);
const STORAGE_DEPOSIT_GAS: Gas = Gas::from_tgas(2);
const MAX_RESULT_LENGTH: usize = "\"+340282366920938463463374607431768211455\"".len(); // u128::MAX

type LstToken = NearToken;

#[near(serializers = [json])]
pub struct Ratio {
    numerator: U128,
    denominator: U128,
}

#[near]
impl LiquidStakingToken {
    /// Returns the number of delegators in the pool.
    pub const fn get_number_of_accounts(&self) -> u64 {
        self.statistics.num_delegators
    }

    /// Returns the current exchange rate of LST to NEAR.
    pub const fn get_exchange_rate(&self) -> Ratio {
        let total_staked = self.statistics.total_staked_amount.as_yoctonear();
        let total_lst_supply = self.token.total_supply;

        let (numerator, denominator) = if total_lst_supply == 0 {
            (1, 1)
        } else {
            (total_staked, total_lst_supply)
        };

        Ratio {
            numerator: U128(numerator),
            denominator: U128(denominator),
        }
    }

    /// Returns the protocol fee fraction as a ratio.
    pub fn get_reward_fee_fraction(&self) -> Ratio {
        Ratio {
            numerator: U128(self.statistics.protocol_fee_bps.into()),
            denominator: U128(stats::BPS_DENOMINATOR.into()),
        }
    }

    /// Returns the public key of the validator that is currently staking.
    pub fn get_staking_key(&self) -> PublicKey {
        self.validator_public_key.clone()
    }

    /// Returns the account ID of the contract owner.
    pub fn get_owner_id(&self) -> AccountId {
        self.owner_id.clone()
    }

    /// Returns the treasury account ID.
    pub fn get_treasury_id(&self) -> AccountId {
        self.treasury_id.clone()
    }

    /// Returns the total NEAR currently backing the LST supply. Grows as
    /// staking rewards are synced.
    pub const fn get_total_staked_balance(&self) -> NearToken {
        self.statistics.total_staked_amount
    }

    /// Returns the sum of NEAR amounts queued for withdrawals.
    pub const fn get_total_pending_withdrawals(&self) -> NearToken {
        self.statistics.total_pending_withdrawals
    }

    /// Returns the total NEAR balance of the pool, including locked balance.
    pub const fn get_total_balance(&self) -> NearToken {
        self.statistics.latest_total_balance
    }

    #[private]
    pub fn modify_state_after_stake(
        &mut self,
        account_id: &AccountId,
        total_staked_tokens: NearToken,
        lst_tokens: LstToken,
        is_stake: bool,
    ) {
        self.statistics.total_staked_amount = total_staked_tokens;

        if is_stake {
            self.internal_deposit(account_id, lst_tokens);
        } else {
            self.internal_withdraw(account_id, lst_tokens);
        }
    }

    /// Publicly callable rewards sync. Reads the contract's `locked` balance
    /// and, when it exceeds the tracked active stake plus pending unstakes,
    /// treats the excess as newly accrued validator rewards that get added to
    /// the LST's backing NEAR (lifting the exchange rate).
    #[pause]
    pub fn ping(&mut self) -> PromiseOrValue<U128> {
        if self.sync_rewards_internal(None).is_zero() {
            return PromiseOrValue::Value(U128(0));
        }

        near_sdk::log!("Rewards synced");

        Promise::new(env::current_account_id())
            .stake(
                self.statistics.total_staked_amount,
                self.validator_public_key.clone(),
            )
            .into()
    }
}

impl LiquidStakingToken {
    /// Implementation of [`Self::sync_rewards`] usable from internal call sites.
    pub(crate) fn sync_rewards_internal(
        &mut self,
        amount_to_exclude: Option<NearToken>,
    ) -> NearToken {
        let current_epoch = env::epoch_height();

        if current_epoch == self.statistics.last_epoch_synced {
            return NearToken::ZERO;
        }

        let current_total_balance = env::account_locked_balance()
            .checked_add(env::account_balance())
            .and_then(|balance| balance.checked_sub(amount_to_exclude.unwrap_or_default()))
            .unwrap_or_else(|| env::panic_str("Overflow while calculating actual total balance"));

        let latest_total_balance = self.statistics.latest_total_balance;

        require!(
            current_total_balance >= latest_total_balance,
            "The new total balance should not be less than the old total balance",
        );

        let total_reward = current_total_balance.saturating_sub(latest_total_balance);

        if total_reward.is_zero() {
            return NearToken::ZERO;
        }

        let (users_reward, fee_near) = self.split_protocol_fee(total_reward);

        self.statistics.increase_stake_amount(users_reward);

        let fee_lst = self.near_to_lst(fee_near);
        self.treasury_deposit(fee_lst);

        self.statistics.increase_stake_amount(fee_near);

        near_sdk::log!(
            "Rewards synced, rewards +{}, protocol fee: +{}, (new total staked amount: {})",
            total_reward.as_yoctonear(),
            fee_near.as_yoctonear(),
            self.statistics.total_staked_amount.as_yoctonear(),
        );

        self.statistics.last_epoch_synced = current_epoch;
        self.statistics.latest_total_balance = current_total_balance;

        total_reward
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

    /// Calculates the protocol fee. Returns an amount without fee and it itself.
    pub(crate) fn split_protocol_fee(&self, amount: NearToken) -> (NearToken, NearToken) {
        if self.statistics.protocol_fee_bps == 0 {
            return (amount, NearToken::ZERO);
        }

        let fee_yocto = mul_div_floor(
            amount.as_yoctonear(),
            u128::from(self.statistics.protocol_fee_bps),
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
