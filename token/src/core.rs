use near_contract_standards::fungible_token::FungibleTokenCore;
use near_sdk::json_types::U128;
use near_sdk::{AccountId, PromiseOrValue, env, near};

use crate::{LiquidStakingToken, LiquidStakingTokenExt};

#[near]
impl FungibleTokenCore for LiquidStakingToken {
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
    pub(crate) fn is_zero_balance(&self, account_id: &AccountId) -> bool {
        self.token.ft_balance_of(account_id.clone()).0 == 0
    }
}
