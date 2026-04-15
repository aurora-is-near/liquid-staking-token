use near_sdk::serde_json::json;
use near_sdk::{
    AccountId, CryptoHash, Gas, GasWeight, NearToken, Promise, PromiseOrValue, env, near, require,
};

use crate::pool::calculate_min_gas;
use crate::pool::unstake::{UnstakeMessage, WithdrawTokens};
use crate::traits::NEAR_DEPOSIT_GAS;
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
            "It's too early to withdraw"
        );

        match args.withdraw_tokens {
            WithdrawTokens::Native => self.withdraw_native(args.receiver_id, *amount, msg_hash),
            WithdrawTokens::Wnear { .. } => self.withdraw_wnear(*amount, args, msg_hash),
        }
    }

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

        let (mut promise, amount_to_withdraw) = if let Some(storage_deposit) = storage_deposit {
            let amount_to_withdraw = NearToken::from_yoctonear(amount)
                .checked_sub(storage_deposit)
                .unwrap_or_else(|| env::panic_str("Storage deposit exceeds the withdrawal amount"));
            (
                Promise::new(self.wnear_id.clone())
                    .function_call("near_deposit", vec![], amount_to_withdraw, NEAR_DEPOSIT_GAS)
                    .function_call(
                        "storage_deposit",
                        json!({
                            "account_id": args.receiver_id,
                            "registration_only": null,
                        })
                        .to_string()
                        .into_bytes(),
                        storage_deposit,
                        Gas::from_tgas(2),
                    ),
                amount_to_withdraw,
            )
        } else {
            let amount_to_withdraw = NearToken::from_yoctonear(amount);
            (
                Promise::new(self.wnear_id.clone()).function_call(
                    "near_deposit",
                    vec![],
                    amount_to_withdraw,
                    NEAR_DEPOSIT_GAS,
                ),
                amount_to_withdraw,
            )
        };

        let is_call = msg.is_some();
        let min_gas = calculate_min_gas(min_gas, is_call);
        let is_call = if let Some(msg) = msg {
            promise = promise.function_call_weight(
                "ft_transfer_call",
                json!({
                    "receiver_id": args.receiver_id,
                    "amount": amount_to_withdraw,
                    "memo": memo,
                    "msg": msg,
                })
                .to_string()
                .as_bytes(),
                ONE_YOCTO,
                min_gas,
                GasWeight(1),
            );

            true
        } else {
            promise = promise.function_call(
                "ft_transfer",
                json!({
                    "receiver_id": args.receiver_id,
                    "amount": amount_to_withdraw,
                    "memo": None::<String>,
                })
                .to_string()
                .as_bytes(),
                ONE_YOCTO,
                min_gas,
            );

            false
        };

        promise.then(
            Self::ext(env::current_account_id())
                .with_unused_gas_weight(1)
                .on_withdraw_transfer(msg_hash, amount_to_withdraw, is_call),
        )
    }

    #[private]
    pub fn on_withdraw_transfer(
        &mut self,
        msg_hash: CryptoHash,
        amount: NearToken,
        is_call: bool,
    ) -> PromiseOrValue<NearToken> {
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

                PromiseOrValue::Value(NearToken::ZERO)
            }
            Err(e) => {
                near_sdk::log!("Error while withdraw transfer: {e}");
                PromiseOrValue::Value(amount)
            }
        }
    }
}
