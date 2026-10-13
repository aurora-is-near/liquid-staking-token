use near_contract_standards::fungible_token::FungibleTokenResolver;
use near_contract_standards::fungible_token::receiver::ext_ft_receiver;
use near_contract_standards::storage_management::ext_storage_management;
use near_plugins::{Pausable, pause};
use near_sdk::json_types::U128;
use near_sdk::{AccountId, Gas, NearToken, Promise, PromiseOrValue, env, near, require};

use crate::pool::unstake::UnstakeTrigger;
use crate::pool::{
    GAS_RESERVE, MODIFY_STATE_AFTER_STAKE_GAS, STORAGE_DEPOSIT_GAS, UnstakeMessage, assert_gas,
    assert_message_size, calculate_min_gas,
};
use crate::storage::FT_STORAGE_DEPOSIT;
use crate::traits::{NEAR_DEPOSIT_GAS, NEAR_WITHDRAW_GAS, ext_wnear};
use crate::{LiquidStakingToken, LiquidStakingTokenExt, ONE_YOCTO};

const REFUND_WNEAR_DEPOSIT_GAS: Gas = Gas::from_tgas(5);
const ON_FT_ON_TRANSFER_GAS: Gas = Gas::from_tgas(40);
const ON_RESTAKE_OR_REFUND_GAS: Gas = Gas::from_tgas(50);

#[derive(Debug, Clone)]
#[near(serializers = [json])]
#[serde(rename_all = "lowercase")]
pub struct StakeMessage {
    /// Account ID to which the staked tokens should be sent.
    pub receiver_id: AccountId,
    /// Message that will be passed to the `ft_transfer_call` callback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<String>,
    /// Memo that will be passed to the `ft_transfer_call` callback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
    /// Flag indicating whether a storage deposit should be attached to the account.
    #[serde(default)]
    pub is_storage_deposit: bool,
    /// Minimum amount of gas that can be used for the `ft_on_transfer` callback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_gas: Option<Gas>,
    /// Message is used in case of an error or refund in the `ft_on_transfer` callback.
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
        assert_message_size(&args);
        assert_gas(stake_gas(&args));
        let sender_id = env::predecessor_account_id();
        let deposit_amount = env::attached_deposit();

        // We don't need to subtract anything when calling from the contract itself.
        let amount_to_exclude = if sender_id == env::current_account_id() {
            NearToken::ZERO
        } else {
            deposit_amount
        };

        self.sync_rewards_internal(Some(amount_to_exclude));
        self.stake_and_deposit(sender_id, args, deposit_amount, DepositToken::Native)
            .unwrap_or_else(|err| env::panic_str(err))
    }

    #[private]
    pub fn on_near_withdraw(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        args: StakeMessage,
    ) -> PromiseOrValue<U128> {
        let deposit_amount = NearToken::from_yoctonear(amount.0);

        if env::promise_result_checked(0, 0).is_err() {
            // The unwrapped NEAR never arrives: undo the booking made on acceptance.
            self.statistics.decrease_total_balance(deposit_amount);
            return PromiseOrValue::Value(amount);
        }

        // Every accepted wNEAR deposit, including this one, is already booked,
        // so unwrapped NEAR cannot be mistaken for a reward here.
        self.sync_rewards_internal(None);

        match self.stake_and_deposit(sender_id, args, deposit_amount, DepositToken::Wnear) {
            Ok(promise) => promise.into(),
            Err(error) => {
                near_sdk::log!("Error occurred while trying to stake and deposit: {error}");
                self.refund_booked_wnear(deposit_amount).into()
            }
        }
    }

    /// `deposit_amount` is the amount to book in the balance tracker: the
    /// native deposit, or zero for wNEAR deposits booked on acceptance.
    #[private]
    pub fn modify_state_before_stake(
        &mut self,
        sender_id: &AccountId,
        receiver_id: &AccountId,
        stake_amount: NearToken,
        deposit_amount: NearToken,
        lst_tokens: NearToken,
    ) {
        // At this point the receiver should be already registered. Panic if not.
        require!(
            self.is_registered(receiver_id),
            format!("The account {receiver_id} is not registered")
        );

        let current_account_id = env::current_account_id();

        if sender_id != &current_account_id {
            self.statistics.increase_total_balance(deposit_amount);
        }

        self.statistics.increase_stake_amount(stake_amount);
        // Here we mint tokens to the contract account id to prevent a withdrawal in a window
        // before a result check of the stake promise. If the result of staking is Ok,
        // then we will transfer minted tokens to the receiver account id
        // in the `on_restake_or_refund` callback.
        self.internal_deposit(&current_account_id, lst_tokens);
    }

    #[private]
    pub fn on_restake_or_refund(
        &mut self,
        sender_id: AccountId,
        stake_amount: NearToken,
        lst_amount: NearToken,
        args: StakeMessage,
        deposit_token: DepositToken,
    ) -> PromiseOrValue<U128> {
        if env::promise_result_checked(0, 0).is_ok() {
            // Check if the receiver is still registered. Return 0 if not.
            if !self.is_registered(&args.receiver_id) {
                return PromiseOrValue::Value(0.into());
            }

            // Move minted LST tokens from the contract account id to the receiver account id.
            self.internal_transfer(&env::current_account_id(), &args.receiver_id, lst_amount);

            // At this point we already staked deposited tokens; therefore, any refund
            // happens via unstake with cooldown period only.
            if let Some(msg) = &args.msg {
                let min_gas = calculate_min_gas(args.min_gas, true);

                ext_ft_receiver::ext(args.receiver_id.clone())
                    .with_static_gas(min_gas)
                    .with_unused_gas_weight(1)
                    .ft_on_transfer(sender_id, lst_amount.as_yoctonear().into(), msg.to_owned())
                    .then(
                        Self::ext(env::current_account_id())
                            .with_static_gas(ON_FT_ON_TRANSFER_GAS)
                            .with_unused_gas_weight(0)
                            .on_ft_on_transfer(lst_amount, args),
                    )
                    .into()
            } else {
                PromiseOrValue::Value(U128(0))
            }
        } else {
            // Rollback state changes:
            self.rollback_state_after_stake(&sender_id, stake_amount, lst_amount);

            if matches!(deposit_token, DepositToken::Native) {
                Promise::new(sender_id).transfer(stake_amount).into()
            } else {
                near_sdk::log!("Error while staking refund wNEAR");
                self.refund_wnear_promise(stake_amount).into()
            }
        }
    }

    #[private]
    pub fn refund_wnear_deposit(&self, deposit_amount: NearToken) -> U128 {
        match env::promise_result_checked(0, 0) {
            Ok(_) => deposit_amount.as_yoctonear().into(),
            Err(e) => {
                near_sdk::log!("Error while wrapping native NEAR to wNEAR: {e}");
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

    #[private]
    pub fn restake_or_refund(
        &mut self,
        sender_id: AccountId,
        deposit_amount: NearToken,
        stake_amount: NearToken,
        lst_amount: NearToken,
        args: StakeMessage,
        deposit_token: DepositToken,
    ) -> Promise {
        let callback_gas = on_restake_or_refund_gas(&args);
        if env::promise_result_checked(0, 0).is_ok() {
            // The batch succeeded, we can continue staking.
            self.restake_promise().then(
                Self::ext(env::current_account_id())
                    .with_unused_gas_weight(1)
                    .with_static_gas(callback_gas)
                    .on_restake_or_refund(sender_id, stake_amount, lst_amount, args, deposit_token),
            )
        } else {
            // At this point the batch, which makes state changes failed; therefore,
            // we don't need to roll back state changes.
            if matches!(deposit_token, DepositToken::Native) {
                env::panic_str("Failed to update state before staking");
            }

            self.refund_booked_wnear(deposit_amount)
        }
    }
}

impl LiquidStakingToken {
    // The method is called by the `ft_on_transfer` callback.
    pub(crate) fn handle_staking(
        &mut self,
        sender_id: AccountId,
        deposit_amount: U128,
        args: StakeMessage,
    ) -> PromiseOrValue<U128> {
        assert_message_size(&args);
        // Reject before burning any wNEAR. The FT resolver can then refund the
        // original tokens without touching another withdrawal's wNEAR residual.
        let callback_gas = stake_gas(&args);
        assert_gas(
            GAS_RESERVE
                .saturating_add(NEAR_WITHDRAW_GAS)
                .saturating_add(callback_gas),
        );
        // Book the deposit once it is accepted. The unwrapped NEAR arrives in a
        // later receipt; until then the tracker runs ahead of the balance, which
        // can only postpone reward recognition, never inflate it.
        self.statistics
            .increase_total_balance(NearToken::from_yoctonear(deposit_amount.0));
        ext_wnear::ext(self.wnear_id.clone())
            .with_attached_deposit(ONE_YOCTO)
            .with_static_gas(NEAR_WITHDRAW_GAS)
            .with_unused_gas_weight(0)
            .near_withdraw(deposit_amount)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(callback_gas)
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
    ) -> Result<Promise, &'static str> {
        let stake_amount = if args.is_storage_deposit {
            deposit_amount
                .checked_sub(FT_STORAGE_DEPOSIT)
                .ok_or("Storage deposit cannot be greater than the staked amount")?
        } else {
            deposit_amount
        };

        if stake_amount == NearToken::ZERO {
            return Err("The amount of NEAR tokens for staking must be more than 0");
        }

        let lst_tokens = self.near_to_lst(stake_amount);
        let restake_gas = GAS_RESERVE.saturating_add(on_restake_or_refund_gas(&args));

        if lst_tokens == NearToken::ZERO {
            return Err("The amount of LST tokens to be minted must be more than 0");
        }

        // wNEAR deposits were booked on acceptance.
        let amount_to_book = match deposit_token {
            DepositToken::Native => deposit_amount,
            DepositToken::Wnear => NearToken::ZERO,
        };

        let mut promise = Promise::new(env::current_account_id());

        // It makes sense in case of staking native NEAR only.
        if matches!(deposit_token, DepositToken::Native) {
            promise = promise
                .refund_to(env::refund_to_account_id())
                .transfer(stake_amount);
        }

        if args.is_storage_deposit {
            promise = ext_storage_management::ext_on(promise)
                .with_attached_deposit(FT_STORAGE_DEPOSIT)
                .with_static_gas(STORAGE_DEPOSIT_GAS)
                .storage_deposit(Some(args.receiver_id.clone()), Some(true));
        }

        Ok(Self::ext_on(promise)
            .with_static_gas(MODIFY_STATE_AFTER_STAKE_GAS)
            .with_unused_gas_weight(0)
            .modify_state_before_stake(
                &sender_id,
                &args.receiver_id,
                stake_amount,
                amount_to_book,
                lst_tokens,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(restake_gas)
                    .with_unused_gas_weight(1)
                    .restake_or_refund(
                        sender_id,
                        deposit_amount,
                        stake_amount,
                        lst_tokens,
                        args,
                        deposit_token,
                    ),
            ))
    }

    /// Returns a wNEAR deposit that was booked on acceptance but not staked.
    /// The `near_deposit` attachment leaves the account in this receipt.
    fn refund_booked_wnear(&mut self, deposit_amount: NearToken) -> Promise {
        self.statistics.decrease_total_balance(deposit_amount);
        self.refund_wnear_promise(deposit_amount)
    }

    fn refund_wnear_promise(&self, deposit_amount: NearToken) -> Promise {
        ext_wnear::ext(self.wnear_id.clone())
            .with_attached_deposit(deposit_amount)
            .with_static_gas(NEAR_DEPOSIT_GAS)
            .with_unused_gas_weight(0)
            .near_deposit()
            .then(
                Self::ext(env::current_account_id())
                    .with_unused_gas_weight(0)
                    .with_static_gas(REFUND_WNEAR_DEPOSIT_GAS)
                    .refund_wnear_deposit(deposit_amount),
            )
    }

    fn rollback_state_after_stake(
        &mut self,
        sender_id: &AccountId,
        stake_amount: NearToken,
        lst_tokens: NearToken,
    ) {
        let current_account_id = env::current_account_id();

        self.internal_withdraw(&current_account_id, lst_tokens);
        self.statistics.decrease_stake_amount(stake_amount);

        if sender_id != &current_account_id {
            // Decrease total_balance by stake_amount (not deposit_amount). If a storage_deposit
            // was requested, those tokens have already been spent on registration and should not
            // be refunded. If no storage_deposit was requested, stake_amount == deposit_amount.
            self.statistics.decrease_total_balance(stake_amount);
        }
    }
}

fn on_restake_or_refund_gas(args: &StakeMessage) -> Gas {
    if args.msg.is_some() {
        GAS_RESERVE
            .saturating_add(calculate_min_gas(args.min_gas, true))
            .saturating_add(ON_FT_ON_TRANSFER_GAS)
    } else {
        ON_RESTAKE_OR_REFUND_GAS
    }
}

fn stake_gas(args: &StakeMessage) -> Gas {
    let gas = GAS_RESERVE
        .saturating_add(MODIFY_STATE_AFTER_STAKE_GAS)
        .saturating_add(GAS_RESERVE)
        .saturating_add(on_restake_or_refund_gas(args));

    if args.is_storage_deposit {
        gas.saturating_add(STORAGE_DEPOSIT_GAS)
    } else {
        gas
    }
}

#[cfg(test)]
#[path = "../security_tests/staking_gas.rs"]
mod security_tests;

#[cfg(test)]
#[path = "../security_tests/concurrent_stakes.rs"]
mod concurrent_stakes_tests;
