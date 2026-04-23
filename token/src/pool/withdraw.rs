use near_contract_standards::fungible_token::core::ext_ft_core;
use near_contract_standards::storage_management::ext_storage_management;
use near_sdk::json_types::U128;
use near_sdk::{AccountId, CryptoHash, NearToken, Promise, env, near, require};

use crate::pool::unstake::{UnstakeMessage, WithdrawTokens};
use crate::pool::{MAX_RESULT_LENGTH, STORAGE_DEPOSIT_GAS, calculate_min_gas};
use crate::traits::{NEAR_DEPOSIT_GAS, ext_wnear};
use crate::{LiquidStakingToken, LiquidStakingTokenExt, ONE_YOCTO};

const UNSTAKE_COOLDOWN_PERIOD: u64 = 4;

#[derive(Debug, Default, Clone, Copy)]
#[near(serializers = [borsh])]
pub struct UserDistribution {
    /// The amount of NEAR queued for withdrawal.
    pub withdrawal_amount: NearToken,
    /// Epoch at which the user initiated the unstake operation.
    pub unstake_epoch: u64,
    /// Whether the user has already deposited storage in the previous withdrawals. The flag
    /// protects against double storage deposits in the case of a partial withdrawal.
    pub storage_already_deposited: bool,
}

#[near]
impl LiquidStakingToken {
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
        gross_amount: NearToken,
        net_amount: NearToken,
        fee: NearToken,
        is_call: bool,
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
                net_amount
            };

            if consumed >= net_amount {
                self.unstake_queue.remove(&msg_hash);
                self.statistics.total_pending_unstake = self
                    .statistics
                    .total_pending_unstake
                    .saturating_sub(gross_amount);
            } else {
                // Partial delivery
                let refund = net_amount.saturating_sub(consumed);

                // Fee proportional to delivered fraction
                let fee_to_keep = NearToken::from_yoctonear(crate::pool::mul_div_floor(
                    fee.as_yoctonear(),
                    consumed.as_yoctonear(),
                    net_amount.as_yoctonear(),
                ));
                let fee_to_reverse = fee.saturating_sub(fee_to_keep);

                self.statistics.withdrawal_collected_fees = self
                    .statistics
                    .withdrawal_collected_fees
                    .checked_sub(fee_to_reverse)
                    .unwrap_or_else(|| env::panic_str("Underflow reversing fees"));

                // Queue stores what the user is still owed in gross NEAR:
                // the undelivered wNEAR value + the reversed fee portion.
                // On re-withdrawal, split_withdrawal_fee will re-apply the fee correctly.
                let remaining = refund.saturating_add(fee_to_reverse);

                let user_distribution = self
                    .unstake_queue
                    .get_mut(&msg_hash)
                    .unwrap_or_else(|| env::panic_str("No withdrawal in unstake queue"));

                user_distribution.withdrawal_amount = remaining;
                // If the user did a storage_deposit, it was successful at this point, and he will
                // not do it again in the next withdrawal.
                user_distribution.storage_already_deposited = true;

                // Pending unstake decreases by the portion that's fully resolved
                self.statistics.total_pending_unstake = self
                    .statistics
                    .total_pending_unstake
                    .saturating_sub(gross_amount.saturating_sub(remaining));
            }
        } else {
            near_sdk::log!("Error while withdrawing wNEAR");
            self.statistics.withdrawal_collected_fees = self
                .statistics
                .withdrawal_collected_fees
                .checked_sub(fee)
                .unwrap_or_else(|| env::panic_str("Underflow reversing fees"));
        }
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
        self.statistics.total_pending_unstake =
            self.statistics.total_pending_unstake.saturating_sub(amount);

        let (net, fee) = self.split_withdrawal_fee(amount);

        self.statistics.withdrawal_collected_fees = self
            .statistics
            .withdrawal_collected_fees
            .saturating_add(fee);

        near_sdk::log!(
            "Withdraw to {receiver_id} gross: {}, net: {}, fee: {}",
            amount.as_yoctonear(),
            net.as_yoctonear(),
            fee.as_yoctonear()
        );

        Promise::new(receiver_id).transfer(net)
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

        let is_distributed_before = self
            .unstake_queue
            .get(&msg_hash)
            .is_some_and(|entry| entry.storage_already_deposited);

        let gross_amount = if is_distributed_before {
            amount
        } else {
            amount
                .checked_sub(storage_deposit.unwrap_or_default())
                .unwrap_or_else(|| env::panic_str("Storage deposit exceeds the withdrawal amount"))
        };

        let (net_amount, fee) = self.split_withdrawal_fee(gross_amount);

        self.statistics.withdrawal_collected_fees = self
            .statistics
            .withdrawal_collected_fees
            .saturating_add(fee);

        let mut promise = ext_wnear::ext(self.wnear_id.clone())
            .with_static_gas(NEAR_DEPOSIT_GAS)
            .with_attached_deposit(net_amount)
            .near_deposit();

        let is_call = if args.receiver_id == env::current_account_id() {
            false
        } else {
            if let Some(storage_deposit) = storage_deposit {
                if !is_distributed_before {
                    promise = ext_storage_management::ext_on(promise)
                        .with_static_gas(STORAGE_DEPOSIT_GAS)
                        .with_attached_deposit(storage_deposit)
                        .storage_deposit(Some(args.receiver_id.clone()), None);
                }
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
                        net_amount.as_yoctonear().into(),
                        memo,
                        msg,
                    );
            } else {
                promise = ext_ft_core::ext_on(promise)
                    .with_attached_deposit(ONE_YOCTO)
                    .with_static_gas(min_gas)
                    .ft_transfer(args.receiver_id, net_amount.as_yoctonear().into(), memo);
            }

            is_call
        };

        promise.then(
            Self::ext(env::current_account_id())
                .with_unused_gas_weight(1)
                .on_withdraw_wnear(msg_hash, amount, net_amount, fee, is_call),
        )
    }
}
