use near_contract_standards::fungible_token::FungibleTokenCore;
use near_plugins::{Pausable, pause};
use near_sdk::json_types::U128;
use near_sdk::{AccountId, NearToken, PromiseOrValue, env, near};

use crate::{LiquidStakingToken, LiquidStakingTokenExt};

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
        let sender_id = env::predecessor_account_id();
        let sender_was_zero = self.is_zero_balance(&sender_id);
        let receiver_was_zero = self.is_zero_balance(&receiver_id);

        let promise = self
            .token
            .ft_transfer_call(receiver_id.clone(), amount, memo, msg);

        // Account for the transfer now; the resolver counts only actual refunds.
        self.update_delegators(&sender_id, sender_was_zero);
        self.update_delegators(&receiver_id, receiver_was_zero);

        promise
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

#[cfg(test)]
#[path = "security_tests/holder_count.rs"]
mod security_tests;
