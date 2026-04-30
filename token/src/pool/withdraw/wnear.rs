use near_contract_standards::fungible_token::core::ext_ft_core;
use near_contract_standards::storage_management::ext_storage_management;
use near_sdk::json_types::U128;
use near_sdk::{CryptoHash, NearToken, Promise, env, near, require};

use crate::pool::withdraw::{ON_WITHDRAW_WNEAR_GAS, REMOVE_LOCK_GAS};
use crate::pool::{
    MAX_RESULT_LENGTH, STORAGE_DEPOSIT_GAS, UnstakeMessage, WithdrawTokens, calculate_min_gas,
};
use crate::traits::{NEAR_DEPOSIT_GAS, ext_wnear};
use crate::{LiquidStakingToken, LiquidStakingTokenExt, ONE_YOCTO};

#[near]
impl LiquidStakingToken {
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
                .unwrap_or_else(|| env::panic_str("No distribution for the given hash"))
                .as_locked_mut()
                .unwrap_or_else(|| {
                    env::panic_str("The user distribution should be locked at this point")
                });

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
}

impl LiquidStakingToken {
    pub(super) fn withdraw_wnear(
        &mut self,
        amount: NearToken,
        args: UnstakeMessage,
        msg_hash: CryptoHash,
    ) -> Promise {
        // Unreachable: `withdraw` only dispatches to this function on the
        // `Wnear` variant. Kept so the compiler can prove the destructure.
        let WithdrawTokens::Wnear {
            storage_deposit,
            msg,
            memo,
            min_gas,
        } = args.withdraw_tokens
        else {
            env::panic_str("Invalid withdraw tokens type");
        };

        let is_self_withdraw = args.receiver_id == env::current_account_id();

        require!(
            !is_self_withdraw || storage_deposit.is_none(),
            "There couldn't be a storage_deposit for the current account withdrawal"
        );

        // Read the per-entry state set by prior attempts:
        //   - wnear_residual: how much of `amount` is already wNEAR at this
        //     contract (refunded back from a prior partial ft_transfer).
        //   - storage_was_paid: whether a prior attempt already registered
        //     `args.receiver_id` on the wNEAR contract.
        let (wnear_residual, storage_was_paid) = self
            .unstake_queue
            .get_mut(&msg_hash)
            .unwrap_or_else(|| env::panic_str("No distribution for the given hash"))
            .as_locked()
            .map_or_else(
                || env::panic_str("The user distribution should be locked at this point"),
                |entry| (entry.wnear_residual, entry.storage_was_paid),
            );

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
