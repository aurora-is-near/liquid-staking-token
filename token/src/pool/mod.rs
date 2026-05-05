use near_plugins::{Pausable, pause};
use near_sdk::json_types::U128;
use near_sdk::{
    AccountId, CryptoHash, Gas, NearToken, Promise, PromiseOrValue, PublicKey, env, near, require,
};
use num_traits::ToPrimitive;

use crate::{LiquidStakingToken, LiquidStakingTokenExt};

pub use stake::StakeMessage;
pub use stats::PoolStatistics;
pub use unstake::{UnstakeMessage, UnstakeTrigger, WithdrawTokens};
pub use withdraw::requests::{Tranche, WithdrawalRequest, WithdrawalRequests};

mod admin;
mod stake;
mod stats;
mod unstake;
mod withdraw;

const FT_STORAGE_DEPOSIT: NearToken = NearToken::from_micronear(1250);
const FT_TRANSFER_GAS: Gas = Gas::from_tgas(2);
const FT_TRANSFER_CALL_GAS_MIN: Gas = Gas::from_tgas(30);
const MODIFY_STATE_AFTER_STAKE_GAS: Gas = Gas::from_tgas(2);
const STORAGE_DEPOSIT_GAS: Gas = Gas::from_tgas(2);
const ON_PING_RESTAKE_GAS: Gas = Gas::from_tgas(20);
const MAX_RESULT_LENGTH: usize = "\"+340282366920938463463374607431768211455\"".len(); // u128::MAX

#[near(serializers = [json])]
pub struct Ratio<T> {
    numerator: T,
    denominator: T,
}

#[near]
impl LiquidStakingToken {
    /// Returns the number of delegators in the pool.
    pub const fn get_number_of_accounts(&self) -> u64 {
        self.statistics.num_delegators
    }

    /// Returns the current exchange rate of LST to NEAR.
    pub const fn get_exchange_rate(&self) -> Ratio<U128> {
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
    pub const fn get_reward_fee_fraction(&self) -> Ratio<u16> {
        Ratio {
            numerator: self.statistics.protocol_fee_bps,
            denominator: stats::BPS_DENOMINATOR,
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

    /// Last reading of `account_locked_balance + account_balance`, refreshed
    /// by reward syncs and balance-affecting calls. Not a live read of the
    /// account's NEAR balance.
    pub const fn get_total_balance(&self) -> NearToken {
        self.statistics.latest_total_balance
    }

    /// Returns every [`Tranche`] currently queued under `hash`, or `None`
    /// if the hash has no entry. Includes both queued (unlocked) tranches
    /// and the in-flight (locked) tranche, if one exists; the lock state is
    /// not exposed through this view. `hash` must be the Keccak-256 of
    /// the borsh-serialized `UnstakeMessage` originally submitted.
    pub fn get_withdrawal_request_tranches(&self, hash: CryptoHash) -> Option<Vec<Tranche>> {
        self.withdrawal_requests
            .get_withdrawal_request_tranches(&hash)
    }

    /// Paginated dump of every queue entry, intended for indexers and admin
    /// dashboards. Returns `[skip, skip + min(limit, MAX_LIMIT))` of the
    /// underlying `IterableMap`'s iteration order; `limit` is silently
    /// clamped to `MAX_LIMIT` (currently 100). Pair with
    /// [`Self::get_withdrawal_requests_count`] for stable totals. Iteration
    /// order is **not stable across removals** (the backing storage uses
    /// swap-remove on its key vector), so a paginating client mid-traversal
    /// may see entries skip or repeat if the queue is mutated.
    pub fn get_withdrawal_requests(&self, skip: usize, limit: usize) -> Vec<WithdrawalRequest> {
        self.withdrawal_requests
            .get_withdrawal_requests(skip, limit)
    }

    /// Paginated list of `hash`es that are currently ready to be
    /// withdrawn — i.e. each has at least one **unlocked, matured** tranche
    /// under it. Intended as the input feed for indexers and frontends that
    /// want to surface "ready to claim" entries to users.
    ///
    /// Hashes whose only matured tranche is already locked (i.e. a
    /// withdrawal is mid-flight for them) are excluded — calling
    /// `withdraw_by_hash` on those would panic with "Unstake request is
    /// already in progress". Hashes with only immature tranches are also
    /// excluded.
    ///
    /// `skip` and `limit` apply to the **filtered** stream; `limit` is
    /// silently clamped to `MAX_LIMIT` (currently 100). Iteration order is
    /// the underlying [`IterableMap`]'s, which is **not stable across
    /// removals**: between paginated calls, completed withdrawals can shift
    /// or remove later entries, so a client traversing with successive
    /// `(skip, limit)` calls may see entries skip or repeat.
    ///
    /// [`IterableMap`]: near_sdk::store::IterableMap
    pub fn get_hashes_available_for_withdrawal(
        &self,
        skip: usize,
        limit: usize,
    ) -> Vec<CryptoHash> {
        self.withdrawal_requests
            .get_hashes_available_for_withdrawal(skip, limit)
    }

    /// Returns the number of distinct `hash` entries currently in the
    /// queue. Each entry is one `(hash, tranches)` pair regardless of
    /// how many tranches sit under that hash.
    pub fn get_withdrawal_requests_count(&self) -> u32 {
        self.withdrawal_requests.len()
    }

    /// Publicly callable rewards sync. Reads `account_locked_balance +
    /// account_balance` and, when it exceeds `latest_total_balance` (the last
    /// recorded total), treats the excess as newly accrued validator rewards
    /// that get added to the LST's backing NEAR (lifting the exchange rate)
    /// and minted as the configured protocol fee to the treasury.
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
            .then(
                Self::ext(env::current_account_id())
                    .with_unused_gas_weight(1)
                    .with_static_gas(ON_PING_RESTAKE_GAS)
                    .on_ping_restake(),
            )
            .into()
    }

    #[private]
    pub fn on_ping_restake(
        &self,
        #[callback_result] result: Result<(), near_sdk::PromiseError>,
    ) -> PromiseOrValue<bool> {
        if result.is_err() && env::account_locked_balance() > NearToken::ZERO {
            near_sdk::log!(
                "Restake failed; unstaking {} yoctoNEAR. Admin recovery required",
                env::account_locked_balance().as_yoctonear()
            );

            Promise::new(env::current_account_id())
                .stake(NearToken::ZERO, self.validator_public_key.clone())
                .into()
        } else {
            PromiseOrValue::Value(true)
        }
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
    /// rate. Uses a 1:1 ratio while no LST has been minted yet (or when the
    /// pool has no backing stake — bootstrap case).
    pub(crate) fn near_to_lst(&self, near_tokens: NearToken) -> NearToken {
        let total_lst = self.token.total_supply;
        let total_staked = self.statistics.total_staked_amount.as_yoctonear();
        let yocto = near_tokens.as_yoctonear();

        if total_lst == 0 || total_staked == 0 {
            return NearToken::from_yoctonear(yocto);
        }
        NearToken::from_yoctonear(mul_div_floor(yocto, total_lst, total_staked))
    }

    /// Converts an LST amount into its NEAR equivalent at the current exchange
    /// rate. Returns zero when no LST has been minted yet.
    pub(crate) fn lst_to_near(&self, lst_tokens: NearToken) -> NearToken {
        let total_lst = self.token.total_supply;
        let total_staked = self.statistics.total_staked_amount.as_yoctonear();
        let yocto = lst_tokens.as_yoctonear();

        if total_lst == 0 {
            return NearToken::ZERO;
        }
        NearToken::from_yoctonear(mul_div_floor(yocto, total_staked, total_lst))
    }

    /// Calculates the protocol fee. Returns a reward without fee and fee itself.
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
            .unwrap_or_else(|| env::panic_str("Protocol fee exceeds the reward amount"));

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
    use ruint::aliases::U256;
    require!(c > 0, "Division by zero in mul_div_floor");

    if let Some(product) = a.checked_mul(b) {
        return product / c;
    }

    // The product overflowed `u128`; promote to `U256`. Any `u128 * u128`
    // fits in `U256`, and the divisor is non-zero (checked above), so the
    // multiplication and division here cannot panic. The only failure mode
    // is the final downcast back to `u128`.
    let result = U256::from(a) * U256::from(b) / U256::from(c);

    result
        .to_u128()
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
