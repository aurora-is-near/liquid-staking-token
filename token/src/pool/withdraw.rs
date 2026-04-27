use near_contract_standards::fungible_token::core::ext_ft_core;
use near_contract_standards::storage_management::ext_storage_management;
use near_plugins::{Pausable, pause};
use near_sdk::json_types::U128;
use near_sdk::{AccountId, CryptoHash, Gas, NearToken, Promise, env, near, require};

use crate::pool::unstake::{UnstakeMessage, WithdrawTokens};
use crate::pool::{MAX_RESULT_LENGTH, STORAGE_DEPOSIT_GAS, calculate_min_gas};
use crate::traits::{NEAR_DEPOSIT_GAS, ext_wnear};
use crate::{LiquidStakingToken, LiquidStakingTokenExt, ONE_YOCTO};

const UNSTAKE_COOLDOWN_PERIOD: u64 = 4;
const ON_WITHDRAW_WNEAR_GAS: Gas = Gas::from_tgas(5);
const ON_WITHDRAW_NATIVE_GAS: Gas = Gas::from_tgas(3);
const REMOVE_LOCK_GAS: Gas = Gas::from_tgas(1);

#[derive(Debug, Default, Clone, Copy)]
#[near(serializers = [borsh])]
pub struct UserDistribution {
    /// The total NEAR-equivalent amount the user can claim from this entry.
    /// `withdrawal_amount - wnear_residual` is still held in NEAR form on the
    /// contract account; `wnear_residual` is already in wNEAR (refunded back
    /// from a prior partial `ft_transfer_call`).
    pub withdrawal_amount: NearToken,
    /// Epoch at which the user initiated the most recent unstake into this
    /// entry. Each subsequent unstake into the same entry resets this and
    /// therefore the cooldown.
    pub unstake_epoch: u64,
    /// How much of `withdrawal_amount` is already held as wNEAR by the
    /// contract account. On the next withdrawal, only the difference
    /// (`withdrawal_amount - wnear_residual`) needs a fresh `near_deposit`.
    /// Invariant: `wnear_residual <= withdrawal_amount`.
    pub wnear_residual: NearToken,
    /// Set after a previous attempt successfully paid `storage_deposit` to
    /// register the receiver on the wNEAR contract. Subsequent retries skip
    /// the storage_deposit step.
    pub storage_was_paid: bool,
}

#[near]
impl LiquidStakingToken {
    #[pause]
    pub fn withdraw(&mut self, args: UnstakeMessage) -> Promise {
        let msg_hash = args
            .hash()
            .unwrap_or_else(|_| env::panic_str("Failed to hash the message"));
        let (amount, epoch) = self.unstake_queue.get(&msg_hash).map_or_else(
            || env::panic_str("Account is not found in the unstake queue"),
            |entry| (&entry.withdrawal_amount, &entry.unstake_epoch),
        );

        require!(
            *epoch + UNSTAKE_COOLDOWN_PERIOD <= env::epoch_height(),
            "The cooldown hasn't passed yet"
        );

        match args.withdraw_tokens {
            WithdrawTokens::Native => Self::withdraw_native(args.receiver_id, *amount, msg_hash),
            WithdrawTokens::Wnear { .. } => self.withdraw_wnear(*amount, args, msg_hash),
        }
    }

    #[private]
    pub fn on_withdraw_native(&mut self, msg_hash: CryptoHash, amount: NearToken) {
        if env::promise_result_checked(0, 0).is_ok() {
            near_sdk::log!("Native NEAR withdrawn successfully");
            self.unstake_queue.remove(&msg_hash);
            self.statistics.decrease_total_balance(amount);
            self.statistics.decrease_pending_withdrawals(amount);
        } else {
            near_sdk::log!("Error while withdrawing Native NEAR");
        }
    }

    #[private]
    pub fn on_withdraw_wnear(
        &mut self,
        msg_hash: CryptoHash,
        amount_to_send: NearToken,
        amount_to_near_deposit: NearToken,
        storage_to_pay: NearToken,
        is_call: bool,
    ) {
        require!(
            env::promise_results_count() == 1,
            "Invalid promise results count"
        );
        let max_len = if is_call { MAX_RESULT_LENGTH } else { 0 };

        if let Ok(bytes) = env::promise_result_checked(0, max_len) {
            // wNEAR's `ft_transfer_call` returns the *used* amount. Cap
            // defensively: a misbehaving receiver returning a value above
            // what was actually sent must not let us under-account the
            // refund.
            let consumed = if is_call {
                near_sdk::serde_json::from_slice::<U128>(&bytes)
                    .map_or_else(
                        |_| env::panic_str("Error while parsing a withdrawal result"),
                        |value| NearToken::from_yoctonear(value.0),
                    )
                    .min(amount_to_send)
            } else {
                // Self-withdraw (no FT step at all) or plain `ft_transfer`
                // (no callback): the full `amount_to_send` was delivered.
                amount_to_send
            };

            let refund = amount_to_send.saturating_sub(consumed);
            let delivered = consumed.saturating_add(storage_to_pay);

            let entry = self
                .unstake_queue
                .get_mut(&msg_hash)
                .unwrap_or_else(|| env::panic_str("No withdrawal in unstake queue"));

            // Decrement the queued claim by what this attempt actually
            // delivered (consumed wNEAR + paid storage). This survives any
            // re-unstake that happened concurrently with the chain — the
            // re-unstake's added claim stays.
            let new_amount = entry
                .withdrawal_amount
                .checked_sub(delivered)
                .unwrap_or_else(|| {
                    env::panic_str("Inconsistent state: delivered exceeds queued claim")
                });

            if new_amount.is_zero() {
                self.unstake_queue.remove(&msg_hash);
            } else {
                entry.withdrawal_amount = new_amount;
                entry.wnear_residual = refund;

                if storage_to_pay > NearToken::ZERO {
                    entry.storage_was_paid = true;
                }
            }

            self.statistics.decrease_pending_withdrawals(delivered);

            // `latest_total_balance` mirrors the contract's locked + unlocked
            // NEAR. Decrement only by what actually moved off the account
            // this attempt (near_deposit + storage_deposit) — never by the
            // delivered wNEAR amount, since the residual portion was already
            // counted on a prior attempt.
            let spent_near = amount_to_near_deposit.saturating_add(storage_to_pay);

            if spent_near > NearToken::ZERO {
                self.statistics.decrease_total_balance(spent_near);
            }
        } else {
            near_sdk::log!("Error while withdrawing wNEAR");
        }
    }

    #[private]
    pub fn remove_lock(&mut self, msg_hash: CryptoHash) {
        self.withdrawal_locks.remove(&msg_hash);
    }
}

impl LiquidStakingToken {
    fn withdraw_native(receiver_id: AccountId, amount: NearToken, msg_hash: CryptoHash) -> Promise {
        near_sdk::log!(
            "Withdraw to {receiver_id} amount: {} yoctoNEAR",
            amount.as_yoctonear(),
        );

        Promise::new(receiver_id).transfer(amount).then(
            Self::ext(env::current_account_id())
                .with_unused_gas_weight(1)
                .with_static_gas(ON_WITHDRAW_NATIVE_GAS)
                .on_withdraw_native(msg_hash, amount),
        )
    }

    fn withdraw_wnear(
        &mut self,
        amount: NearToken,
        args: UnstakeMessage,
        msg_hash: CryptoHash,
    ) -> Promise {
        let WithdrawTokens::Wnear {
            storage_deposit,
            msg,
            memo,
            min_gas,
        } = args.withdraw_tokens
        else {
            env::panic_str("Invalid withdraw tokens type");
        };

        require!(
            args.receiver_id != env::current_account_id() || storage_deposit.is_none(),
            "There couldn't be a storage_deposit for the current account withdrawal"
        );

        require!(
            !self.withdrawal_locks.contains(&msg_hash),
            "The withdrawal for this hash is already in progress"
        );

        self.withdrawal_locks.insert(msg_hash);

        // Read the per-entry state set by prior attempts:
        //   - wnear_residual: how much of `amount` is already wNEAR at this
        //     contract (refunded back from a prior partial ft_transfer).
        //   - storage_was_paid: whether a prior attempt already registered
        //     `args.receiver_id` on the wNEAR contract.
        let (wnear_residual, storage_was_paid) = self
            .unstake_queue
            .get(&msg_hash)
            .map_or((NearToken::ZERO, false), |entry| {
                (entry.wnear_residual, entry.storage_was_paid)
            });

        // Storage_deposit is paid at most once per queue entry — only when
        // the user requested one and a prior attempt didn't already pay it.
        let storage_to_pay = if storage_was_paid {
            NearToken::ZERO
        } else {
            storage_deposit.unwrap_or_default()
        };

        // wNEAR amount to deliver via ft_transfer this attempt (the user's
        // full claim minus what we earmark for receiver registration).
        let amount_to_send = amount
            .checked_sub(storage_to_pay)
            .unwrap_or_else(|| env::panic_str("Storage deposit exceeds the withdrawal amount"));

        // Of `amount_to_send`, the residual is already held as wNEAR; only
        // the difference still needs a fresh `near_deposit`.
        let amount_to_near_deposit = amount_to_send
            .checked_sub(wnear_residual)
            .unwrap_or_else(|| env::panic_str("wNEAR residual exceeds the deliverable amount"));

        let is_self_withdraw = args.receiver_id == env::current_account_id();
        let is_call = !is_self_withdraw && msg.is_some();
        let ft_min_gas = calculate_min_gas(min_gas, is_call);
        let ft_amount: U128 = amount_to_send.as_yoctonear().into();

        // Build the chain conditionally. Each step is included only if it has
        // real work to do, so we never schedule an empty receipt:
        //   1. near_deposit(amount_to_near_deposit) — if any NEAR to convert.
        //   2. storage_deposit(storage_to_pay)      — if non-zero and not self.
        //   3. ft_transfer[_call](amount_to_send)   — if not self.
        let mut chain = (amount_to_near_deposit > NearToken::ZERO).then(|| {
            ext_wnear::ext(self.wnear_id.clone())
                .with_static_gas(NEAR_DEPOSIT_GAS)
                .with_attached_deposit(amount_to_near_deposit)
                .near_deposit()
        });

        if storage_to_pay > NearToken::ZERO && !is_self_withdraw {
            let next = chain.map_or_else(
                || ext_storage_management::ext(self.wnear_id.clone()),
                ext_storage_management::ext_on,
            );

            chain = Some(
                next.with_static_gas(STORAGE_DEPOSIT_GAS)
                    .with_attached_deposit(storage_to_pay)
                    .storage_deposit(Some(args.receiver_id.clone()), None),
            );
        }

        if !is_self_withdraw {
            let next = chain.map_or_else(
                || ext_ft_core::ext(self.wnear_id.clone()),
                ext_ft_core::ext_on,
            );

            let p = if let Some(msg) = msg {
                next.with_attached_deposit(ONE_YOCTO)
                    .with_static_gas(ft_min_gas)
                    .with_unused_gas_weight(1)
                    .ft_transfer_call(args.receiver_id, ft_amount, memo, msg)
            } else {
                next.with_attached_deposit(ONE_YOCTO)
                    .with_static_gas(ft_min_gas)
                    .ft_transfer(args.receiver_id, ft_amount, memo)
            };

            chain = Some(p);
        }

        let promise = chain.unwrap_or_else(|| {
            // Reachable only on a degenerate self-withdraw retry where the
            // claim is already fully in wNEAR at the contract — but a
            // self-withdraw never produces a partial delivery, so this
            // should be unreachable in practice.
            env::panic_str(
                "Nothing to deliver: claim is already fully held as wNEAR by the contract",
            )
        });

        promise
            .then(
                Self::ext(env::current_account_id())
                    .with_unused_gas_weight(1)
                    .with_static_gas(ON_WITHDRAW_WNEAR_GAS)
                    .on_withdraw_wnear(
                        msg_hash,
                        amount_to_send,
                        amount_to_near_deposit,
                        storage_to_pay,
                        is_call,
                    ),
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(REMOVE_LOCK_GAS)
                    .remove_lock(msg_hash),
            )
    }
}
