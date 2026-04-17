use near_contract_standards::fungible_token::core::ext_ft_core;
use near_contract_standards::storage_management::ext_storage_management;
use near_sdk::json_types::U128;
use near_sdk::{AccountId, CryptoHash, NearToken, Promise, PromiseOrValue, env, near, require};

use crate::pool::unstake::{UnstakeMessage, WithdrawTokens};
use crate::pool::{STORAGE_DEPOSIT_GAS, calculate_min_gas};
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

        match args.withdraw_tokens {
            WithdrawTokens::Native => self.withdraw_native(args.receiver_id, *amount, msg_hash),
            WithdrawTokens::Wnear { .. } => self.withdraw_wnear(*amount, args, msg_hash),
        }
    }

    #[private]
    pub fn on_withdraw_transfer(
        &mut self,
        msg_hash: CryptoHash,
        amount: NearToken,
        is_call: bool,
    ) -> PromiseOrValue<U128> {
        require!(
            env::promise_results_count() == 1,
            "Invalid promise results count"
        );
        let max_len = if is_call { 44 } else { 0 };

        match env::promise_result_checked(0, max_len) {
            Ok(bytes) => {
                if is_call {
                    let consumed = near_sdk::serde_json::from_slice::<NearToken>(&bytes)
                        .unwrap_or_else(|_| {
                            env::panic_str("Error while parsing withdrawal result");
                        });

                    if consumed < amount {
                        // TODO: Handle this case.
                        near_sdk::log!("Withdrawal result is less than the amount");
                    }
                }

                near_sdk::log!("Withdraw successful");
                self.unstake_queue.remove(&msg_hash);

                PromiseOrValue::Value(0.into())
            }
            Err(e) => {
                near_sdk::log!("Error while withdraw transfer: {e}");
                PromiseOrValue::Value(amount.as_yoctonear().into())
            }
        }
    }
}

impl LiquidStakingToken {
    fn withdraw_native(
        &mut self,
        receiver_id: AccountId,
        amount: u128,
        msg_hash: CryptoHash,
    ) -> Promise {
        self.unstake_queue.remove(&msg_hash);
        Promise::new(receiver_id).transfer(NearToken::from_yoctonear(amount))
    }

    fn withdraw_wnear(&self, amount: u128, args: UnstakeMessage, msg_hash: CryptoHash) -> Promise {
        let WithdrawTokens::Wnear {
            storage_deposit,
            msg,
            memo,
            min_gas,
        } = args.withdraw_tokens
        else {
            env::panic_str("Invalid withdraw tokens type");
        };

        let amount_to_withdraw = NearToken::from_yoctonear(amount)
            .checked_sub(storage_deposit.unwrap_or_default())
            .unwrap_or_else(|| env::panic_str("Storage deposit exceeds the withdrawal amount"));

        let mut promise = ext_wnear::ext(self.wnear_id.clone())
            .with_static_gas(NEAR_DEPOSIT_GAS)
            .with_attached_deposit(amount_to_withdraw)
            .near_deposit();

        let is_call = if args.receiver_id != env::current_account_id() {
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
        } else {
            false
        };

        promise.then(
            Self::ext(env::current_account_id())
                .with_unused_gas_weight(1)
                .on_withdraw_transfer(msg_hash, amount_to_withdraw, is_call),
        )
    }
}
