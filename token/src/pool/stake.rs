use near_contract_standards::fungible_token::FungibleTokenResolver;
use near_contract_standards::fungible_token::receiver::ext_ft_receiver;
use near_contract_standards::storage_management::ext_storage_management;
use near_sdk::json_types::U128;
use near_sdk::{AccountId, Gas, NearToken, Promise, PromiseOrValue, env, near, require};

use crate::pool::{
    MODIFY_STAKED_AMOUNT_GAS, STORAGE_DEPOSIT_GAS, UnstakeMessage, calculate_min_gas,
};
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
    /// The maximum amount of gas that can be used for the `ft_on_transfer` callback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_gas: Option<Gas>,
    /// A message is used in case of an error or refund in the `ft_on_transfer` callback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refund_message: Option<UnstakeMessage>,
}

#[near(serializers = [json])]
pub enum DepositToken {
    Native,
    Wnear,
}

#[near]
impl LiquidStakingToken {
    /// Allow staking tokens by depositing attached native NEAR to the contract to itself
    /// or to another optional account to the corresponding direction.
    #[payable]
    pub fn stake(&mut self, args: StakeMessage) -> Promise {
        let amount_to_stake = env::attached_deposit();
        self.stake_and_deposit(amount_to_stake, args, DepositToken::Native)
    }

    #[private]
    pub fn on_near_withdraw(&mut self, amount: U128, args: StakeMessage) -> Promise {
        require!(
            env::promise_result_checked(0, 0).is_ok(),
            "Failed to withdraw NEAR from wNEAR"
        );

        self.stake_and_deposit(
            NearToken::from_yoctonear(amount.0),
            args,
            DepositToken::Wnear,
        )
    }

    #[private]
    pub fn on_stake_and_deposit(
        &mut self,
        amount_to_stake: NearToken,
        amount_staked_tokens: NearToken,
        args: StakeMessage,
        deposit_token: DepositToken,
    ) -> PromiseOrValue<U128> {
        match env::promise_result_checked(0, 0) {
            Ok(_) => {
                if let Some(msg) = &args.msg {
                    let min_gas = calculate_min_gas(args.min_gas, true);

                    ext_ft_receiver::ext(args.receiver_id.clone())
                        .with_static_gas(min_gas)
                        .with_unused_gas_weight(1)
                        .ft_on_transfer(
                            env::current_account_id(),
                            amount_staked_tokens.as_yoctonear().into(),
                            msg.to_string(),
                        )
                        .then(
                            Self::ext(env::current_account_id())
                                .with_unused_gas_weight(1)
                                .on_ft_on_transfer(amount_staked_tokens, args),
                        )
                        .into()
                } else {
                    PromiseOrValue::Value(U128(0))
                }
            }
            Err(_) => match deposit_token {
                DepositToken::Native => env::panic_str("Error while staking native NEAR"),
                DepositToken::Wnear => ext_wnear::ext(self.wnear_id.clone())
                    .with_attached_deposit(amount_to_stake)
                    .with_static_gas(NEAR_DEPOSIT_GAS)
                    .with_unused_gas_weight(1)
                    .near_deposit()
                    .then(
                        Self::ext(env::current_account_id())
                            .with_unused_gas_weight(1)
                            .refund_wnear_deposit(amount_to_stake),
                    )
                    .into(),
            },
        }
    }

    #[private]
    pub fn refund_wnear_deposit(&mut self, amount: NearToken) -> PromiseOrValue<U128> {
        match env::promise_result_checked(0, 0) {
            Ok(_) => PromiseOrValue::Value(amount.as_yoctonear().into()),
            Err(e) => {
                near_sdk::log!("Error while depositing near to wNEAR: {e}");
                PromiseOrValue::Value(0.into())
            }
        }
    }

    #[private]
    pub fn on_ft_on_transfer(
        &mut self,
        amount_shared_tokens: NearToken,
        args: StakeMessage,
    ) -> PromiseOrValue<U128> {
        // The refund message wasn't attached, so consider the result of the `ft_on_transfer`
        // as successful without refunding.
        if args.refund_message.is_none() {
            return PromiseOrValue::Value(0.into());
        }

        let consumed_shared_tokens = self.token.ft_resolve_transfer(
            env::current_account_id(),
            args.receiver_id.clone(),
            amount_shared_tokens.as_yoctonear().into(),
        );

        let refund_shared_tokens = amount_shared_tokens
            .saturating_sub(NearToken::from_yoctonear(consumed_shared_tokens.0));

        if refund_shared_tokens.is_zero() {
            return PromiseOrValue::Value(0.into());
        }

        // TODO: Recalculate the refund of shared tokens to near regarding the locked balance.
        let refund_near = refund_shared_tokens;

        let unstake_msg = args
            .refund_message
            .unwrap_or_else(|| env::panic_str("The refund message is invalid or doesn't exist"));

        self.handle_unstaking(
            args.receiver_id,
            refund_near.as_yoctonear().into(),
            unstake_msg,
        )
        .into()
    }
}

impl LiquidStakingToken {
    // The method is called by the `ft_on_transfer` callback.
    pub(crate) fn handle_staking(
        &self,
        _sender_id: AccountId,
        amount: U128,
        args: StakeMessage,
    ) -> PromiseOrValue<U128> {
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
        deposit_token: DepositToken,
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
            .refund_to(env::refund_to_account_id())
            .transfer(env::attached_deposit())
            .stake(new_total_staked_amount, self.validator_public_key.clone());

        if let Some(storage_deposit) = args.storage_deposit {
            promise = ext_storage_management::ext_on(promise)
                .with_attached_deposit(storage_deposit)
                .with_static_gas(STORAGE_DEPOSIT_GAS)
                .storage_deposit(Some(args.receiver_id.clone()), Some(false));
        }

        promise = Self::ext_on(promise)
            .with_static_gas(MODIFY_STAKED_AMOUNT_GAS)
            .with_unused_gas_weight(0)
            .modify_total_staked_amount(
                &args.receiver_id,
                new_total_staked_amount,
                staked_tokens,
                true,
            );

        promise.then(
            Self::ext(env::current_account_id())
                .with_unused_gas_weight(1)
                .on_stake_and_deposit(amount, staked_tokens, args, deposit_token),
        )
    }
}
