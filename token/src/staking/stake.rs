use near_sdk::json_types::U128;
use near_sdk::serde_json::json;
use near_sdk::{
    AccountId, Gas, GasWeight, NearToken, Promise, PromiseOrValue, env, near, require, serde_json,
};

use crate::staking::{MODIFY_STAKED_AMOUNT_GAS, calculate_min_gas};
use crate::traits::{NEAR_DEPOSIT_GAS, NEAR_WITHDRAW_GAS, ext_wnear};
use crate::{LiquidStakingToken, LiquidStakingTokenExt, ONE_YOCTO};

#[derive(Debug, Clone)]
#[near(serializers = [json])]
#[serde(rename_all = "lowercase")]
pub struct StakeMessage {
    /// The account ID to which the staked tokens should be sent.
    pub receiver_id: AccountId,
    /// A message that will be passed to the `ft_transfer_call` callback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<String>,
    /// A memo that will be passed to the `ft_transfer_call` callback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
    /// The amount of storage deposit to be attached to the account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_deposit: Option<NearToken>,
    /// The maximum amount of gas that can be used for the.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_gas: Option<Gas>,
}

#[near]
impl LiquidStakingToken {
    /// Allow staking tokens by depositing attached native NEAR to the contract to itself
    /// or to another optional account to the corresponding direction.
    #[payable]
    pub fn stake(&mut self, args: StakeMessage) -> PromiseOrValue<U128> {
        let amount_to_stake = env::attached_deposit();
        self.stake_and_deposit(amount_to_stake, args, Some(env::predecessor_account_id()))
    }

    // The method is called by the `ft_on_transfer` callback.
    pub(crate) fn handle_staking(
        &self,
        _sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        let args = serde_json::from_str::<StakeMessage>(&msg)
            .unwrap_or_else(|_| env::panic_str("Invalid format of the message"));

        ext_wnear::ext(self.wnear_id.clone())
            .with_attached_deposit(ONE_YOCTO)
            .with_static_gas(NEAR_WITHDRAW_GAS)
            .near_withdraw(amount)
            .then(
                Self::ext(env::current_account_id())
                    .with_unused_gas_weight(1)
                    .on_near_withdraw(amount, args),
            )
            .into()
    }

    #[private]
    pub fn on_near_withdraw(&mut self, amount: U128, args: StakeMessage) -> PromiseOrValue<U128> {
        require!(
            env::promise_result_checked(0, 0).is_ok(),
            "Failed to withdraw NEAR from wNEAR"
        );

        self.stake_and_deposit(NearToken::from_yoctonear(amount.0), args, None)
    }

    pub(crate) fn stake_and_deposit(
        &mut self,
        amount: NearToken,
        args: StakeMessage,
        refund_to: Option<AccountId>,
    ) -> PromiseOrValue<U128> {
        let stake_amount = amount
            .checked_sub(args.storage_deposit.unwrap_or_default())
            .unwrap_or_else(|| {
                env::panic_str("Storage deposit cannot be greater than the staked amount")
            });

        // TODO: Recalculate the amount_staked_tokens regarding the locked balance
        let amount_staked_token = stake_amount;

        self.token.internal_deposit(
            &env::current_account_id(),
            amount_staked_token.as_yoctonear(),
        );

        let new_locked_balance = env::account_locked_balance()
            .checked_add(stake_amount)
            .unwrap_or_else(|| env::panic_str("Overflow while calculating new locked balance"));
        let new_total_staked_amount = self
            .total_staked_amount
            .checked_add(stake_amount)
            .unwrap_or_else(|| {
                env::panic_str("Overflow while calculating new total staked amount")
            });

        let mut promise = Promise::new(env::current_account_id())
            .stake(new_locked_balance, self.validator_public_key.clone())
            .function_call_weight(
                "modify_total_staked_amount".to_string(),
                json!({
                    "amount": new_total_staked_amount,
                })
                .to_string()
                .into_bytes(),
                NearToken::ZERO,
                MODIFY_STAKED_AMOUNT_GAS,
                GasWeight(0),
            );

        if let Some(storage_deposit) = args.storage_deposit {
            promise = promise.function_call(
                "storage_deposit".to_string(),
                json!({
                    "account_id": args.receiver_id,
                    "registration_only": false,
                })
                .to_string()
                .into_bytes(),
                storage_deposit,
                Gas::from_tgas(2),
            );
        }

        let is_call = args.msg.is_some();
        let min_gas = calculate_min_gas(args.min_gas, is_call);

        if let Some(msg) = args.msg {
            promise = promise.function_call_weight(
                "ft_transfer_call".to_string(),
                json!({
                    "receiver_id": args.receiver_id,
                    "amount": amount_staked_token,
                    "memo": args.memo,
                    "msg": msg,
                })
                .to_string()
                .into_bytes(),
                ONE_YOCTO,
                min_gas,
                GasWeight(1),
            );
        } else {
            promise = promise.function_call(
                "ft_transfer".to_string(),
                json!({
                    "receiver_id": args.receiver_id,
                    "amount": amount_staked_token,
                })
                .to_string()
                .into_bytes(),
                ONE_YOCTO,
                min_gas,
            );
        }

        promise
            .then(
                Self::ext(env::current_account_id())
                    .with_unused_gas_weight(1)
                    .on_stake_and_deposit(amount, amount_staked_token, refund_to, is_call),
            )
            .into()
    }

    #[private]
    pub fn on_stake_and_deposit(
        &mut self,
        amount_to_stake: NearToken,
        amount_staked_token: NearToken,
        refund_to: Option<AccountId>,
        is_call: bool,
    ) -> PromiseOrValue<U128> {
        let max_len = if is_call { 44 } else { 0 };
        let refund_staked_tokens = match env::promise_result_checked(0, max_len) {
            Ok(bytes) => {
                if is_call {
                    let refund = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                        near_sdk::log!("Failed to parse the refund amount");
                        NearToken::ZERO
                    });

                    if !refund.is_zero() {
                        near_sdk::log!("Partial refund of staked tokens: {}", refund);
                    }

                    // TODO: Find out what to do with the partial refund of staked tokens.
                }

                NearToken::ZERO
            }
            Err(e) => {
                near_sdk::log!("Error while staking: {e}");
                amount_staked_token
            }
        };

        if refund_staked_tokens.is_zero() {
            PromiseOrValue::Value(U128(0))
        } else {
            self.token.internal_withdraw(
                &env::current_account_id(),
                refund_staked_tokens.as_yoctonear(),
            );
            // Recalculate the refund of staked tokens to near if case of partial refund of staked tokens.
            let refund_near = amount_to_stake;
            refund_to.map_or_else(
                || {
                    ext_wnear::ext(self.wnear_id.clone())
                        .with_attached_deposit(refund_near)
                        .with_static_gas(NEAR_DEPOSIT_GAS)
                        .with_unused_gas_weight(1)
                        .near_deposit()
                        .then(
                            Self::ext(env::current_account_id())
                                .with_unused_gas_weight(1)
                                .refund_wnear_deposit(refund_near),
                        )
                        .into()
                },
                |account_id| {
                    Promise::new(account_id).transfer(refund_near).detach();
                    PromiseOrValue::Value(U128(0))
                },
            )
        }
    }

    #[private]
    pub fn refund_wnear_deposit(&mut self, amount: NearToken) -> PromiseOrValue<NearToken> {
        match env::promise_result_checked(0, 0) {
            Ok(_) => PromiseOrValue::Value(amount),
            Err(e) => {
                near_sdk::log!("Error while depositing near to wNEAR: {e}");
                PromiseOrValue::Value(NearToken::ZERO)
            }
        }
    }
}
