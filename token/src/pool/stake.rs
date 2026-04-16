use near_sdk::json_types::U128;
use near_sdk::{
    AccountId, Gas, NearToken, Promise, PromiseOrValue, env, near, require, serde_json,
};

use crate::pool::{MODIFY_STAKED_AMOUNT_GAS, STORAGE_DEPOSIT_GAS, calculate_min_gas};
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
    pub fn stake(&mut self, args: StakeMessage) -> Promise {
        let amount_to_stake = env::attached_deposit();
        self.stake_and_deposit(amount_to_stake, args, Some(env::predecessor_account_id()))
    }

    #[private]
    pub fn on_near_withdraw(&mut self, amount: U128, args: StakeMessage) -> Promise {
        require!(
            env::promise_result_checked(0, 0).is_ok(),
            "Failed to withdraw NEAR from wNEAR"
        );

        self.stake_and_deposit(NearToken::from_yoctonear(amount.0), args, None)
    }

    #[private]
    pub fn on_stake_and_deposit(
        &mut self,
        amount_to_stake: NearToken,
        amount_staked_tokens: NearToken,
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
                amount_staked_tokens
            }
        };

        if refund_staked_tokens.is_zero() {
            PromiseOrValue::Value(U128(0))
        } else {
            // Recalculate the refund of staked tokens to near if case of partial refund of staked tokens.
            let refund_near = amount_to_stake;
            refund_to
                .map_or_else(
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
                    },
                    |account_id| Promise::new(account_id).transfer(refund_near),
                )
                .into()
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

impl LiquidStakingToken {
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
            .with_unused_gas_weight(0)
            .near_withdraw(amount)
            .then(
                Self::ext(env::current_account_id())
                    .with_unused_gas_weight(1)
                    .on_near_withdraw(amount, args),
            )
            .into()
    }

    pub(crate) fn stake_and_deposit(
        &self,
        amount: NearToken,
        args: StakeMessage,
        refund_to: Option<AccountId>,
    ) -> Promise {
        let stake_amount = amount
            .checked_sub(args.storage_deposit.unwrap_or_default())
            .unwrap_or_else(|| {
                env::panic_str("Storage deposit cannot be greater than the staked amount")
            });

        // TODO: Recalculate the amount_staked_tokens regarding the locked balance
        let staked_tokens = stake_amount;

        let new_total_staked_amount = self
            .total_staked_amount
            .checked_add(stake_amount)
            .unwrap_or_else(|| {
                env::panic_str("Overflow while calculating new total staked amount")
            });

        let mut promise = Promise::new(env::current_account_id())
            // The problem here is that attached 1 yoctoNEAR to `ft_transfer_*` is included
            // in the return value, and the user gets more by 1 yoctoNEAR than they attached.
            // .refund_to(env::refund_to_account_id())
            // .transfer(env::attached_deposit())
            .stake(new_total_staked_amount, self.validator_public_key.clone());

        promise = Self::ext_on(promise)
            .with_static_gas(MODIFY_STAKED_AMOUNT_GAS)
            .with_unused_gas_weight(0)
            .modify_total_staked_amount(new_total_staked_amount, staked_tokens, true);

        if let Some(storage_deposit) = args.storage_deposit {
            promise = Self::ext_on(promise)
                .with_attached_deposit(storage_deposit)
                .with_static_gas(STORAGE_DEPOSIT_GAS)
                .storage_deposit(Some(args.receiver_id.clone()), Some(false));
        }

        let is_call = args.msg.is_some();
        let min_gas = calculate_min_gas(args.min_gas, is_call);

        if let Some(msg) = args.msg {
            promise = Self::ext_on(promise)
                .with_attached_deposit(ONE_YOCTO)
                .with_static_gas(min_gas)
                .with_unused_gas_weight(1)
                .ft_transfer_call(
                    args.receiver_id,
                    staked_tokens.as_yoctonear().into(),
                    args.memo,
                    msg,
                );
        } else {
            promise = Self::ext_on(promise)
                .with_attached_deposit(ONE_YOCTO)
                .with_static_gas(min_gas)
                .ft_transfer(
                    args.receiver_id,
                    staked_tokens.as_yoctonear().into(),
                    args.memo,
                );
        }

        promise.then(
            Self::ext(env::current_account_id())
                .with_unused_gas_weight(1)
                .on_stake_and_deposit(amount, staked_tokens, refund_to, is_call),
        )
    }
}
