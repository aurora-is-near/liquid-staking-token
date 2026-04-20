use near_plugins::{AccessControllable, access_control_any};
use near_sdk::{AccountId, Gas, NearToken, Promise, PublicKey, env, near, require};

use crate::pool::math::mul_div_floor;
use crate::{
    BPS_DENOMINATOR, LiquidStakingToken, LiquidStakingTokenExt, MAX_WITHDRAWAL_FEE_BPS, Role,
};

pub use stake::StakeMessage;
pub use unstake::{UnstakeMessage, WithdrawTokens};

mod math;
mod stake;
mod unstake;
mod withdraw;

const FT_TRANSFER_GAS: Gas = Gas::from_tgas(2);
const FT_TRANSFER_CALL_GAS_MIN: Gas = Gas::from_tgas(30);
const MODIFY_STAKED_AMOUNT_GAS: Gas = Gas::from_tgas(1);
const STORAGE_DEPOSIT_GAS: Gas = Gas::from_tgas(2);
const MAX_RESULT_LENGTH: usize = "\"+340282366920938463463374607431768211455\"".len(); // u128::MAX

#[near]
impl LiquidStakingToken {
    #[allow(clippy::missing_const_for_fn)]
    pub fn get_number_of_accounts(&self) -> u64 {
        1
    }

    pub fn get_reward_fee_fraction(&self) -> near_sdk::serde_json::Value {
        near_sdk::serde_json::json!({ "numerator": 1, "denominator": 10 })
    }

    pub fn get_staking_key(&self) -> PublicKey {
        self.validator_public_key.clone()
    }

    pub fn get_owner_id(&self) -> AccountId {
        self.owner_id.clone()
    }

    /// Returns the total NEAR currently backing the LST supply. Grows as
    /// staking rewards are synced.
    pub const fn get_total_staked_amount(&self) -> NearToken {
        self.total_staked_amount
    }

    /// Returns the sum of NEAR amounts queued for withdrawal (pre- and
    /// post-cooldown).
    pub const fn get_total_pending_unstake(&self) -> NearToken {
        self.total_pending_unstake
    }

    /// Returns the current withdrawal fee in basis points (1 bp = 0.01%).
    pub const fn get_withdrawal_fee_bps(&self) -> u16 {
        self.withdrawal_fee_bps
    }

    /// Returns the NEAR amount accumulated from withdrawal fees, awaiting claim.
    pub const fn get_withdrawal_fees_collected(&self) -> NearToken {
        self.withdrawal_fees_collected
    }

    /// Returns the current LST ↔ NEAR exchange rate as `{numerator, denominator}`,
    /// where `1 LST = numerator / denominator` NEAR.
    ///
    /// Returns `1 / 1` while the LST supply is zero (bootstrap ratio).
    pub fn get_exchange_rate(&self) -> near_sdk::serde_json::Value {
        let supply = self.token.total_supply;
        let backing = self.total_staked_amount.as_yoctonear();
        if supply == 0 {
            near_sdk::serde_json::json!({ "numerator": "1", "denominator": "1" })
        } else {
            near_sdk::serde_json::json!({
                "numerator": backing.to_string(),
                "denominator": supply.to_string(),
            })
        }
    }

    /// Publicly callable rewards sync. Reads the contract's `locked` balance
    /// and, when it exceeds the tracked active stake plus pending unstakes,
    /// treats the excess as newly accrued validator rewards that get added to
    /// the LST's backing NEAR (lifting the exchange rate).
    ///
    /// Returns the NEAR amount of rewards recognized by this call.
    pub fn sync_rewards(&mut self) -> NearToken {
        self.sync_rewards_internal()
    }

    /// Updates the withdrawal fee. Capped at [`MAX_WITHDRAWAL_FEE_BPS`].
    #[access_control_any(roles(Role::Admin))]
    pub fn set_withdrawal_fee_bps(&mut self, fee_bps: u16) {
        require!(
            fee_bps <= MAX_WITHDRAWAL_FEE_BPS,
            "withdrawal_fee_bps exceeds MAX_WITHDRAWAL_FEE_BPS"
        );
        self.withdrawal_fee_bps = fee_bps;
    }

    /// Transfers accumulated withdrawal fees to `receiver_id` as native NEAR.
    /// If `amount` is `None`, the entire collected balance is claimed.
    #[access_control_any(roles(Role::Admin))]
    pub fn claim_withdrawal_fees(
        &mut self,
        receiver_id: AccountId,
        amount: Option<NearToken>,
    ) -> Promise {
        let amount = amount.unwrap_or(self.withdrawal_fees_collected);
        require!(!amount.is_zero(), "Nothing to claim");
        require!(
            amount <= self.withdrawal_fees_collected,
            "Requested amount exceeds collected fees"
        );

        self.withdrawal_fees_collected = self
            .withdrawal_fees_collected
            .checked_sub(amount)
            .unwrap_or_else(|| env::panic_str("Underflow while claiming withdrawal fees"));

        Promise::new(receiver_id).transfer(amount)
    }

    #[private]
    #[allow(clippy::missing_const_for_fn)]
    pub fn modify_total_staked_amount(
        &mut self,
        account_id: &AccountId,
        total_staked_amount: NearToken,
        shared_tokens: NearToken,
        is_stake: bool,
    ) {
        self.total_staked_amount = total_staked_amount;

        if is_stake {
            self.token
                .internal_deposit(account_id, shared_tokens.as_yoctonear());
        } else {
            self.token
                .internal_withdraw(account_id, shared_tokens.as_yoctonear());
        }
    }
}

impl LiquidStakingToken {
    /// Converts a NEAR amount into the LST mint amount at the current exchange
    /// rate. Uses a 1:1 ratio while no LST has been minted yet.
    pub(crate) fn near_to_lst(&self, near_amount: NearToken) -> NearToken {
        let supply = self.token.total_supply;
        let backing = self.total_staked_amount.as_yoctonear();
        let yocto = if supply == 0 || backing == 0 {
            near_amount.as_yoctonear()
        } else {
            mul_div_floor(near_amount.as_yoctonear(), supply, backing)
        };
        NearToken::from_yoctonear(yocto)
    }

    /// Converts an LST amount into its NEAR equivalent at the current exchange
    /// rate. Returns zero when no LST has been minted yet.
    pub(crate) fn lst_to_near(&self, lst_amount: NearToken) -> NearToken {
        let supply = self.token.total_supply;
        let backing = self.total_staked_amount.as_yoctonear();
        if supply == 0 {
            return NearToken::ZERO;
        }
        NearToken::from_yoctonear(mul_div_floor(lst_amount.as_yoctonear(), backing, supply))
    }

    /// Applies the configured withdrawal fee. Returns `(amount_net, fee)`.
    pub(crate) fn split_withdrawal_fee(&self, amount: NearToken) -> (NearToken, NearToken) {
        if self.withdrawal_fee_bps == 0 {
            return (amount, NearToken::ZERO);
        }
        let fee_yocto = mul_div_floor(
            amount.as_yoctonear(),
            u128::from(self.withdrawal_fee_bps),
            u128::from(BPS_DENOMINATOR),
        );
        let fee = NearToken::from_yoctonear(fee_yocto);
        let net = amount
            .checked_sub(fee)
            .unwrap_or_else(|| env::panic_str("Withdrawal fee exceeds withdrawal amount"));
        (net, fee)
    }

    /// Implementation of [`Self::sync_rewards`] usable from internal call sites.
    pub(crate) fn sync_rewards_internal(&mut self) -> NearToken {
        let locked = env::account_locked_balance();
        let expected = self
            .total_staked_amount
            .saturating_add(self.total_pending_unstake);

        if locked <= expected {
            return NearToken::ZERO;
        }

        let rewards = locked.saturating_sub(expected);
        self.total_staked_amount = self
            .total_staked_amount
            .checked_add(rewards)
            .unwrap_or_else(|| env::panic_str("Overflow while adding rewards"));
        near_sdk::log!(
            "Rewards synced: +{} yoctoNEAR (new backing: {})",
            rewards.as_yoctonear(),
            self.total_staked_amount.as_yoctonear(),
        );
        rewards
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
