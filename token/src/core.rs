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
        let is_new_delegator = self.is_zero_balance(&receiver_id);

        self.token.ft_transfer(receiver_id, amount, memo);

        if is_new_delegator {
            self.statistics.increase_delegators();
        }

        if self.is_zero_balance(&env::predecessor_account_id()) {
            self.statistics.decrease_delegators();
        }
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
        let is_new_delegator = self.is_zero_balance(&receiver_id);
        let promise = self.token.ft_transfer_call(receiver_id, amount, memo, msg);

        // Asymmetric on purpose: increment for a newly-funded receiver runs
        // synchronously here because the receiver's post-call balance is
        // known. The matching decrement for the sender (and the receiver, on
        // a full refund) is deferred to `ft_resolve_transfer`, which sees the
        // final balances after `ft_on_transfer` has reported its `unused`
        // amount. Tightening this into a synchronous decrement would
        // double-count once the resolver runs.
        if is_new_delegator {
            self.statistics.increase_delegators();
        }

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
    pub(crate) fn treasury_deposit(&mut self, amount: NearToken) {
        let treasury_id = self.treasury_id.clone();
        self.internal_deposit(&treasury_id, amount);
    }

    pub(crate) fn internal_deposit(&mut self, account_id: &AccountId, amount: NearToken) {
        if self.is_zero_balance(account_id) {
            self.statistics.increase_delegators();
        }

        self.token
            .internal_deposit(account_id, amount.as_yoctonear());
    }

    pub(crate) fn internal_withdraw(&mut self, account_id: &AccountId, amount: NearToken) {
        self.token
            .internal_withdraw(account_id, amount.as_yoctonear());

        if self.is_zero_balance(account_id) {
            self.statistics.decrease_delegators();
        }
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
}
