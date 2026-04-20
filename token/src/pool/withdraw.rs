use near_contract_standards::fungible_token::core::ext_ft_core;
use near_contract_standards::storage_management::ext_storage_management;
use near_sdk::json_types::U128;
use near_sdk::{AccountId, CryptoHash, NearToken, Promise, PromiseOrValue, env, near, require};

use crate::pool::unstake::{UnstakeMessage, WithdrawTokens};
use crate::pool::{MAX_RESULT_LENGTH, STORAGE_DEPOSIT_GAS, calculate_min_gas};
use crate::traits::{NEAR_DEPOSIT_GAS, ext_wnear};
use crate::{LiquidStakingToken, LiquidStakingTokenExt, ONE_YOCTO};

const UNSTAKE_COOLDOWN_PERIOD: u64 = 4;

#[near]
impl LiquidStakingToken {
    pub fn withdraw(&mut self, args: UnstakeMessage) -> Promise {
        let msg_hash = args
            .hash()
            .unwrap_or_else(|_| env::panic_str("Failed to hash the message"));
        let (amount, epoch) = self
            .unstake_queue
            .get(&msg_hash)
            .unwrap_or_else(|| env::panic_str("Account is not found in the unstake queue"));

        require!(
            *epoch + UNSTAKE_COOLDOWN_PERIOD <= env::epoch_height(),
            "The cooldown hasn't passed yet"
        );

        let gross = NearToken::from_yoctonear(*amount);

        match args.withdraw_tokens {
            WithdrawTokens::Native => self.withdraw_native(args.receiver_id, gross, msg_hash),
            WithdrawTokens::Wnear { .. } => self.withdraw_wnear(gross, args, msg_hash),
        }
    }

    #[private]
    pub fn on_withdraw_wnear(
        &mut self,
        msg_hash: CryptoHash,
        gross: NearToken,
        amount: NearToken,
        is_call: bool,
    ) -> PromiseOrValue<U128> {
        require!(
            env::promise_results_count() == 1,
            "Invalid promise results count"
        );
        let max_len = if is_call { MAX_RESULT_LENGTH } else { 0 };

        let refund = match env::promise_result_checked(0, max_len) {
            Ok(bytes) => {
                if is_call {
                    let consumed = near_sdk::serde_json::from_slice::<U128>(&bytes).map_or_else(
                        |_| env::panic_str("Error while parsing withdrawal result"),
                        |value| NearToken::from_yoctonear(value.0),
                    );

                    let refund = amount.checked_sub(consumed).unwrap_or_else(|| {
                        env::panic_str("Consumed amount exceeds the withdrawal amount")
                    });

                    if refund.is_zero() {
                        self.unstake_queue.remove(&msg_hash);
                        self.total_pending_unstake =
                            self.total_pending_unstake.saturating_sub(gross);
                    } else {
                        let (amount_to_withdraw, _) = self.unstake_queue.get_mut(&msg_hash)
                            .unwrap_or_else(|| env::panic_str("There is no withdrawal in the unstake queue for the given message hash"));
                        *amount_to_withdraw = refund.as_yoctonear();

                        let consumed_from_queue = gross.checked_sub(refund).unwrap_or_else(|| {
                            env::panic_str("Refund exceeds the gross withdrawal amount")
                        });
                        self.total_pending_unstake = self
                            .total_pending_unstake
                            .saturating_sub(consumed_from_queue);
                    }

                    refund
                } else {
                    self.unstake_queue.remove(&msg_hash);
                    self.total_pending_unstake =
                        self.total_pending_unstake.saturating_sub(gross);
                    NearToken::ZERO
                }
            }
            Err(e) => {
                near_sdk::log!("Error while withdraw transfer: {e}");
                // Full failure: wNEAR transfer never took effect. Restore the
                // originally-deducted fee so the user can retry withdrawal for
                // the full gross amount.
                let fee = gross.saturating_sub(amount);
                if !fee.is_zero() {
                    self.withdrawal_fees_collected =
                        self.withdrawal_fees_collected.saturating_sub(fee);
                }
                amount
            }
        };

        PromiseOrValue::Value(refund.as_yoctonear().into())
    }
}

impl LiquidStakingToken {
    fn withdraw_native(
        &mut self,
        receiver_id: AccountId,
        gross: NearToken,
        msg_hash: CryptoHash,
    ) -> Promise {
        self.unstake_queue.remove(&msg_hash);
        self.total_pending_unstake = self.total_pending_unstake.saturating_sub(gross);

        let (net, fee) = self.split_withdrawal_fee(gross);
        self.withdrawal_fees_collected = self.withdrawal_fees_collected.saturating_add(fee);

        Promise::new(receiver_id).transfer(net)
    }

    fn withdraw_wnear(
        &mut self,
        gross: NearToken,
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

        let amount_after_storage = gross
            .checked_sub(storage_deposit.unwrap_or_default())
            .unwrap_or_else(|| env::panic_str("Storage deposit exceeds the withdrawal amount"));

        // Apply the withdrawal fee to the user-destined portion (not the
        // storage deposit, which is forwarded to wNEAR for registration).
        let (amount_to_withdraw, fee) = self.split_withdrawal_fee(amount_after_storage);
        self.withdrawal_fees_collected = self.withdrawal_fees_collected.saturating_add(fee);

        let mut promise = ext_wnear::ext(self.wnear_id.clone())
            .with_static_gas(NEAR_DEPOSIT_GAS)
            .with_attached_deposit(amount_to_withdraw)
            .near_deposit();

        let is_call = if args.receiver_id == env::current_account_id() {
            false
        } else {
            if let Some(storage_deposit) = storage_deposit {
                promise = ext_storage_management::ext_on(promise)
                    .with_static_gas(STORAGE_DEPOSIT_GAS)
                    .with_attached_deposit(storage_deposit)
                    .storage_deposit(Some(args.receiver_id.clone()), None);
            }

            let is_call = msg.is_some();
            let min_gas = calculate_min_gas(min_gas, is_call);

            if let Some(msg) = msg {
                promise = ext_ft_core::ext_on(promise)
                    .with_attached_deposit(ONE_YOCTO)
                    .with_static_gas(min_gas)
                    .with_unused_gas_weight(1)
                    .ft_transfer_call(
                        args.receiver_id,
                        amount_to_withdraw.as_yoctonear().into(),
                        memo,
                        msg,
                    );
            } else {
                promise = ext_ft_core::ext_on(promise)
                    .with_attached_deposit(ONE_YOCTO)
                    .with_static_gas(min_gas)
                    .ft_transfer(
                        args.receiver_id,
                        amount_to_withdraw.as_yoctonear().into(),
                        memo,
                    );
            }

            is_call
        };

        promise.then(
            Self::ext(env::current_account_id())
                .with_unused_gas_weight(1)
                .on_withdraw_wnear(msg_hash, gross, amount_to_withdraw, is_call),
        )
    }
}
