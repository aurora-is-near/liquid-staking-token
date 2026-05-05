use near_contract_standards::fungible_token::FungibleTokenResolver;
use near_contract_standards::fungible_token::receiver::ext_ft_receiver;
use near_contract_standards::storage_management::ext_storage_management;
use near_plugins::{Pausable, pause};
use near_sdk::json_types::U128;
use near_sdk::{AccountId, Gas, NearToken, Promise, PromiseOrValue, env, near, require};

use crate::pool::unstake::UnstakeTrigger;
use crate::pool::{
    MODIFY_STATE_AFTER_STAKE_GAS, STORAGE_DEPOSIT_GAS, UnstakeMessage, calculate_min_gas,
};
use crate::traits::{NEAR_DEPOSIT_GAS, NEAR_WITHDRAW_GAS, ext_wnear};
use crate::{LiquidStakingToken, LiquidStakingTokenExt, ONE_YOCTO};

const REFUND_WNEAR_DEPOSIT_GAS: Gas = Gas::from_tgas(1);

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

#[derive(Debug, Clone, Copy)]
#[near(serializers = [json])]
pub enum DepositToken {
    Native,
    Wnear,
}

#[near]
impl LiquidStakingToken {
    /// Allow staking tokens by depositing attached native NEAR to the contract to itself
    /// or to another optional account to the corresponding direction.
    #[pause]
    #[payable]
    pub fn stake(&mut self, args: StakeMessage) -> Promise {
        let sender_id = env::predecessor_account_id();
        let deposit_amount = env::attached_deposit();
        let is_contract_staking = sender_id == env::current_account_id();

        // We don't need to subtract anything when calling from the contract itself.
        self.sync_rewards_internal(Some(if is_contract_staking {
            NearToken::ZERO
        } else {
            deposit_amount
        }));

        self.stake_and_deposit(
            sender_id,
            args,
            deposit_amount,
            DepositToken::Native,
            is_contract_staking,
        )
    }

    #[private]
    pub fn on_near_withdraw(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        args: StakeMessage,
    ) -> Promise {
        require!(
            env::promise_result_checked(0, 0).is_ok(),
            "Failed to withdraw NEAR from wNEAR"
        );

        let stake_amount = NearToken::from_yoctonear(amount.0);

        self.sync_rewards_internal(Some(stake_amount));
        self.stake_and_deposit(sender_id, args, stake_amount, DepositToken::Wnear, false)
    }

    #[private]
    pub fn modify_state_after_stake(
        &mut self,
        account_id: &AccountId,
        stake_amount: NearToken,
        deposit_amount: NearToken,
        lst_tokens: NearToken,
        is_contract_staking: bool,
    ) {
        if !is_contract_staking {
            self.statistics.increase_total_balance(deposit_amount);
        }

        self.statistics.increase_stake_amount(stake_amount);
        self.internal_deposit(account_id, lst_tokens);
    }

    #[private]
    pub fn on_stake_and_deposit(
        &mut self,
        sender_id: AccountId,
        deposit_amount: NearToken,
        lst_tokens: NearToken,
        args: StakeMessage,
    ) -> PromiseOrValue<U128> {
        match env::promise_result_checked(0, 0) {
            Ok(_) => {
                // At this point we already staked deposited tokens; therefore, any refund
                // happens via unstake with cooldown period only.
                if let Some(msg) = &args.msg {
                    let min_gas = calculate_min_gas(args.min_gas, true);

                    ext_ft_receiver::ext(args.receiver_id.clone())
                        .with_static_gas(min_gas)
                        .with_unused_gas_weight(1)
                        .ft_on_transfer(
                            sender_id,
                            lst_tokens.as_yoctonear().into(),
                            msg.to_string(),
                        )
                        .then(
                            Self::ext(env::current_account_id())
                                .with_unused_gas_weight(1)
                                .on_ft_on_transfer(lst_tokens, args),
                        )
                        .into()
                } else {
                    PromiseOrValue::Value(U128(0))
                }
            }
            // Reachable only if `modify_state_after_stake` (the immediate
            // predecessor) panics — the underlying `stake` action's failure
            // does not propagate here. Kept as a defensive recovery path.
            Err(_) => ext_wnear::ext(self.wnear_id.clone())
                .with_attached_deposit(deposit_amount)
                .with_static_gas(NEAR_DEPOSIT_GAS)
                .with_unused_gas_weight(1)
                .near_deposit()
                .then(
                    Self::ext(env::current_account_id())
                        .with_unused_gas_weight(0)
                        .with_static_gas(REFUND_WNEAR_DEPOSIT_GAS)
                        .refund_wnear_deposit(deposit_amount),
                )
                .into(),
        }
    }

    #[private]
    pub fn refund_wnear_deposit(&self, deposit_amount: NearToken) -> U128 {
        match env::promise_result_checked(0, 0) {
            Ok(_) => deposit_amount.as_yoctonear().into(),
            Err(e) => {
                near_sdk::log!("Error while depositing near to wNEAR: {e}");
                0.into()
            }
        }
    }

    #[private]
    pub fn on_ft_on_transfer(
        &mut self,
        lst_tokens: NearToken,
        args: StakeMessage,
    ) -> PromiseOrValue<U128> {
        // No refund message means the receiver's `ft_on_transfer` result is
        // accepted as-is, with no automatic recovery.
        let Some(unstake_msg) = args.refund_message else {
            return PromiseOrValue::Value(0.into());
        };

        let consumed = self
            .ft_resolve_transfer(
                env::current_account_id(),
                args.receiver_id.clone(),
                lst_tokens.as_yoctonear().into(),
            )
            .0;

        let refund = lst_tokens.saturating_sub(NearToken::from_yoctonear(consumed));

        if refund.is_zero() {
            return PromiseOrValue::Value(0.into());
        }

        near_sdk::log!(
            "Received refund {} of LST tokens from {}, initiate unstaking",
            refund.as_yoctonear(),
            args.receiver_id
        );

        self.handle_unstaking(refund, unstake_msg, UnstakeTrigger::RefundByContract)
            .into()
    }
}

impl LiquidStakingToken {
    // The method is called by the `ft_on_transfer` callback.
    pub(crate) fn handle_staking(
        &self,
        sender_id: AccountId,
        deposit_amount: U128,
        args: StakeMessage,
    ) -> PromiseOrValue<U128> {
        ext_wnear::ext(self.wnear_id.clone())
            .with_attached_deposit(ONE_YOCTO)
            .with_static_gas(NEAR_WITHDRAW_GAS)
            .with_unused_gas_weight(0)
            .near_withdraw(deposit_amount)
            .then(
                Self::ext(env::current_account_id())
                    .with_unused_gas_weight(1)
                    .on_near_withdraw(sender_id, deposit_amount, args),
            )
            .into()
    }

    pub(crate) fn stake_and_deposit(
        &self,
        sender_id: AccountId,
        args: StakeMessage,
        deposit_amount: NearToken,
        deposit_token: DepositToken,
        is_contract_staking: bool,
    ) -> Promise {
        let stake_amount = deposit_amount
            .checked_sub(args.storage_deposit.unwrap_or_default())
            .unwrap_or_else(|| {
                env::panic_str("Storage deposit cannot be greater than the staked amount")
            });

        require!(
            stake_amount > NearToken::ZERO,
            "The amount of NEAR tokens for staking must be more than 0"
        );

        let lst_tokens = self.near_to_lst(stake_amount);

        require!(
            lst_tokens > NearToken::ZERO,
            "The amount of LST tokens to be minted must be more than 0"
        );

        let new_total_staked_amount = self
            .statistics
            .total_staked_amount
            .checked_add(stake_amount)
            .unwrap_or_else(|| {
                env::panic_str("Overflow while calculating new total staked amount")
            });

        let mut promise = Promise::new(env::current_account_id())
            .refund_to(env::refund_to_account_id())
            .transfer(stake_amount)
            .stake(new_total_staked_amount, self.validator_public_key.clone());

        if let Some(storage_deposit) = args.storage_deposit {
            promise = ext_storage_management::ext_on(promise)
                .with_attached_deposit(storage_deposit)
                .with_static_gas(STORAGE_DEPOSIT_GAS)
                .storage_deposit(Some(args.receiver_id.clone()), Some(false));
        }

        promise = Self::ext_on(promise)
            .with_static_gas(MODIFY_STATE_AFTER_STAKE_GAS)
            .with_unused_gas_weight(0)
            .modify_state_after_stake(
                &args.receiver_id,
                stake_amount,
                deposit_amount,
                lst_tokens,
                is_contract_staking,
            );

        // LST tokens will be moved via `ft_transfer` and deposited tokens are native NEAR
        if args.msg.is_none() && matches!(deposit_token, DepositToken::Native) {
            promise
        } else {
            promise.then(
                Self::ext(env::current_account_id())
                    .with_unused_gas_weight(1)
                    .on_stake_and_deposit(sender_id, deposit_amount, lst_tokens, args),
            )
        }
    }
}
