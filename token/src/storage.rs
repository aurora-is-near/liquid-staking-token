use near_contract_standards::storage_management::{
    StorageBalance, StorageBalanceBounds, StorageManagement,
};
use near_sdk::{AccountId, NearToken, env, near};

use crate::{LiquidStakingToken, LiquidStakingTokenExt, ONE_YOCTO};

#[near]
impl StorageManagement for LiquidStakingToken {
    #[payable]
    fn storage_deposit(
        &mut self,
        account_id: Option<AccountId>,
        registration_only: Option<bool>,
    ) -> StorageBalance {
        let is_external = env::current_account_id() != env::predecessor_account_id();
        let target = account_id
            .clone()
            .unwrap_or_else(env::predecessor_account_id);
        let is_registered = self.token.storage_balance_of(target).is_some();
        let result = self.token.storage_deposit(account_id, registration_only);

        if is_external && !is_registered {
            self.statistics
                .increase_total_balance(self.token.storage_balance_bounds().min);
        }

        result
    }

    #[payable]
    fn storage_withdraw(&mut self, amount: Option<NearToken>) -> StorageBalance {
        let balance = self.token.storage_withdraw(amount);
        let is_external = env::current_account_id() != env::predecessor_account_id();

        if is_external {
            self.statistics.increase_total_balance(ONE_YOCTO);
        }

        balance
    }

    #[payable]
    fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        let unregistered = self.token.storage_unregister(force);

        if env::current_account_id() != env::predecessor_account_id() {
            if unregistered {
                self.statistics
                    .decrease_total_balance(self.token.storage_balance_bounds().min);
            } else {
                self.statistics.increase_total_balance(ONE_YOCTO);
            }
        }

        unregistered
    }

    fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        self.token.storage_balance_bounds()
    }

    fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        self.token.storage_balance_of(account_id)
    }
}
