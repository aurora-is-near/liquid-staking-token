use near_contract_standards::fungible_token::FungibleTokenCore;
use near_contract_standards::fungible_token::receiver::ext_ft_receiver;
use near_plugins::{Pausable, pause};
use near_sdk::json_types::U128;
use near_sdk::{AccountId, Gas, NearToken, PromiseOrValue, env, near};

use crate::pool::{GAS_RESERVE, assert_gas};
use crate::{LiquidStakingToken, LiquidStakingTokenExt};

const FT_RESOLVE_TRANSFER_GAS: Gas = Gas::from_tgas(15);

#[near]
impl FungibleTokenCore for LiquidStakingToken {
    #[pause]
    #[payable]
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        self.transfer_tokens(&receiver_id, amount, memo);
    }

    #[pause]
    #[payable]
    fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<U128> {
        assert_gas(GAS_RESERVE.saturating_add(FT_RESOLVE_TRANSFER_GAS));
        let sender_id = env::predecessor_account_id();
        let sender_was_zero = self.is_zero_balance(&sender_id);
        let receiver_was_zero = self.is_zero_balance(&receiver_id);

        self.token.ft_transfer(receiver_id.clone(), amount, memo);
        let promise = ext_ft_receiver::ext(receiver_id.clone())
            .with_unused_gas_weight(1)
            .ft_on_transfer(sender_id.clone(), amount, msg)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(FT_RESOLVE_TRANSFER_GAS)
                    .with_unused_gas_weight(0)
                    .ft_resolve_transfer(sender_id.clone(), receiver_id.clone(), amount),
            );

        // Account for the transfer now; the resolver counts only actual refunds.
        self.update_delegators(&sender_id, sender_was_zero);
        self.update_delegators(&receiver_id, receiver_was_zero);

        promise.into()
    }

    fn ft_total_supply(&self) -> U128 {
        self.token.ft_total_supply()
    }

    fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        self.token.ft_balance_of(account_id)
    }
}

impl LiquidStakingToken {
    fn transfer_tokens(&mut self, receiver_id: &AccountId, amount: U128, memo: Option<String>) {
        let sender_id = env::predecessor_account_id();
        let sender_was_zero = self.is_zero_balance(&sender_id);
        let receiver_was_zero = self.is_zero_balance(receiver_id);
        self.token.ft_transfer(receiver_id.clone(), amount, memo);
        self.update_delegators(&sender_id, sender_was_zero);
        self.update_delegators(receiver_id, receiver_was_zero);
    }

    pub(crate) fn treasury_deposit(&mut self, amount: NearToken) {
        let treasury_id = self.treasury_id.clone();
        self.internal_deposit(&treasury_id, amount);
    }

    pub(crate) fn internal_deposit(&mut self, account_id: &AccountId, amount: NearToken) {
        let was_zero = self.is_zero_balance(account_id);

        self.token
            .internal_deposit(account_id, amount.as_yoctonear());
        self.update_delegators(account_id, was_zero);
    }

    pub(crate) fn internal_withdraw(&mut self, account_id: &AccountId, amount: NearToken) {
        let was_zero = self.is_zero_balance(account_id);

        self.token
            .internal_withdraw(account_id, amount.as_yoctonear());
        self.update_delegators(account_id, was_zero);
    }

    pub(crate) fn internal_transfer(
        &mut self,
        sender_id: &AccountId,
        receiver_id: &AccountId,
        amount: NearToken,
    ) {
        self.internal_withdraw(sender_id, amount);
        self.internal_deposit(receiver_id, amount);
    }

    pub(crate) fn is_zero_balance(&self, account_id: &AccountId) -> bool {
        self.token.ft_balance_of(account_id.clone()).0 == 0
    }

    pub(crate) fn update_delegators(&mut self, account_id: &AccountId, was_zero: bool) {
        match (was_zero, self.is_zero_balance(account_id)) {
            (true, false) => self.statistics.increase_delegators(),
            (false, true) => self.statistics.decrease_delegators(),
            _ => {}
        }
    }
}
