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
const REMOVE_LOCK_GAS: Gas = Gas::from_tgas(1);

#[derive(Debug, Default, Clone, Copy)]
#[near(serializers = [borsh])]
pub struct UserDistribution {
    /// The amount of NEAR queued for withdrawal.
    pub withdrawal_amount: NearToken,
    /// Epoch at which the user initiated the unstake operation.
    pub unstake_epoch: u64,
    /// Marks the queue entry as carrying residual wNEAR already held by the contract
    /// from a previous partial delivery. On retry, both `near_deposit` and
    /// `storage_deposit` must be skipped: the wNEAR is already deposited (refunded
    /// back via `ft_resolve_transfer`), and the receiver is already registered.
    pub is_distributed_before: bool,
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
            WithdrawTokens::Native => self.withdraw_native(args.receiver_id, *amount, msg_hash),
            WithdrawTokens::Wnear { .. } => self.withdraw_wnear(*amount, args, msg_hash),
        }
    }

    #[private]
    pub fn on_withdraw_wnear(
        &mut self,
        msg_hash: CryptoHash,
        total_amount: NearToken, // The amount could include storage_deposit.
        withdrawal_amount: NearToken, // The amount without storage_deposit.
        is_call: bool,
        is_distributed_before: bool,
    ) {
        require!(
            env::promise_results_count() == 1,
            "Invalid promise results count"
        );
        let max_len = if is_call { MAX_RESULT_LENGTH } else { 0 };

        if let Ok(bytes) = env::promise_result_checked(0, max_len) {
            let consumed = if is_call {
                near_sdk::serde_json::from_slice::<U128>(&bytes).map_or_else(
                    |_| env::panic_str("Error while parsing a withdrawal result"),
                    |value| NearToken::from_yoctonear(value.0),
                )
            } else {
                withdrawal_amount
            };

            let actual_amount = if consumed >= withdrawal_amount {
                self.unstake_queue.remove(&msg_hash);
                total_amount
            } else {
                // Partial delivery: keep the undelivered wnear as the user's residual claim.
                let refund = withdrawal_amount.saturating_sub(consumed);
                let user_distribution = self
                    .unstake_queue
                    .get_mut(&msg_hash)
                    .unwrap_or_else(|| env::panic_str("No withdrawal in unstake queue"));

                user_distribution.withdrawal_amount = refund;
                // Storage was already paid on this attempt; don't charge again.
                user_distribution.is_distributed_before = true;

                total_amount.saturating_sub(refund)
            };

            self.statistics.decrease_pending_withdrawals(actual_amount);

            // We need to subtract the withdrawal amount from the total balance only once.
            if !is_distributed_before {
                self.statistics.decrease_total_balance(total_amount);
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
    fn withdraw_native(
        &mut self,
        receiver_id: AccountId,
        amount: NearToken,
        msg_hash: CryptoHash,
    ) -> Promise {
        self.unstake_queue.remove(&msg_hash);
        self.statistics.decrease_total_balance(amount);
        self.statistics.decrease_pending_withdrawals(amount);

        near_sdk::log!(
            "Withdraw to {receiver_id} amount: {} yoctoNEAR",
            amount.as_yoctonear(),
        );

        Promise::new(receiver_id).transfer(amount)
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

        let is_distributed_before = self
            .unstake_queue
            .get(&msg_hash)
            .is_some_and(|entry| entry.is_distributed_before);

        let amount_without_storage = if is_distributed_before {
            amount
        } else {
            amount
                .checked_sub(storage_deposit.unwrap_or_default())
                .unwrap_or_else(|| env::panic_str("Storage deposit exceeds the withdrawal amount"))
        };

        let mut promise = if is_distributed_before {
            Promise::new(self.wnear_id.clone())
        } else {
            // We must call the near_deposit only once.
            let promise = ext_wnear::ext(self.wnear_id.clone())
                .with_static_gas(NEAR_DEPOSIT_GAS)
                .with_attached_deposit(amount_without_storage)
                .near_deposit();

            if let Some(storage_deposit) = storage_deposit {
                ext_storage_management::ext_on(promise)
                    .with_static_gas(STORAGE_DEPOSIT_GAS)
                    .with_attached_deposit(storage_deposit)
                    .storage_deposit(Some(args.receiver_id.clone()), None)
            } else {
                promise
            }
        };

        let is_call = if args.receiver_id == env::current_account_id() {
            false
        } else {
            let is_call = msg.is_some();
            let min_gas = calculate_min_gas(min_gas, is_call);

            if let Some(msg) = msg {
                promise = ext_ft_core::ext_on(promise)
                    .with_attached_deposit(ONE_YOCTO)
                    .with_static_gas(min_gas)
                    .with_unused_gas_weight(1)
                    .ft_transfer_call(
                        args.receiver_id,
                        amount_without_storage.as_yoctonear().into(),
                        memo,
                        msg,
                    );
            } else {
                promise = ext_ft_core::ext_on(promise)
                    .with_attached_deposit(ONE_YOCTO)
                    .with_static_gas(min_gas)
                    .ft_transfer(
                        args.receiver_id,
                        amount_without_storage.as_yoctonear().into(),
                        memo,
                    );
            }

            is_call
        };

        promise
            .then(
                Self::ext(env::current_account_id())
                    .with_unused_gas_weight(1)
                    .with_static_gas(ON_WITHDRAW_WNEAR_GAS)
                    .on_withdraw_wnear(
                        msg_hash,
                        amount,
                        amount_without_storage,
                        is_call,
                        is_distributed_before,
                    ),
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(REMOVE_LOCK_GAS)
                    .remove_lock(msg_hash),
            )
    }
}
