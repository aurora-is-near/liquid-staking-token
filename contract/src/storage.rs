// Storage management is intentionally **not** gated by `#[pause]`: account
// (un)registration must always be available so users can recover storage
// deposits and exit the contract even when staking flows are paused.
//
// Each method updates `latest_total_balance` only for *external* calls — i.e.
// when the predecessor isn't the contract itself. Internal self-calls during
// the stake/withdraw chains are no-ops in NEAR-balance terms (a self-call
// transfers from the account to itself), so crediting them would drift the
// reward-sync tracker.
use near_contract_standards::storage_management::{
    StorageBalance, StorageBalanceBounds, StorageManagement,
};
use near_sdk::{AccountId, NearToken, env, near};

use crate::{LiquidStakingToken, LiquidStakingTokenExt, ONE_YOCTO};

pub const FT_STORAGE_DEPOSIT: NearToken = NearToken::from_micronear(1250);

#[near]
impl StorageManagement for LiquidStakingToken {
    #[payable]
    fn storage_deposit(
        &mut self,
        account_id: Option<AccountId>,
        registration_only: Option<bool>,
    ) -> StorageBalance {
        let target = account_id
            .clone()
            .unwrap_or_else(env::predecessor_account_id);
        let was_registered = self.token.storage_balance_of(target).is_some();
        let result = self.token.storage_deposit(account_id, registration_only);

        if is_external_call() && !was_registered {
            self.statistics
                .increase_total_balance(self.token.storage_balance_bounds().min);
        }

        result
    }

    #[payable]
    fn storage_withdraw(&mut self, amount: Option<NearToken>) -> StorageBalance {
        let balance = self.token.storage_withdraw(amount);

        if is_external_call() {
            self.statistics.increase_total_balance(ONE_YOCTO);
        }

        balance
    }

    #[payable]
    fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        let unregistered = self.token.storage_unregister(force);

        if is_external_call() {
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

impl LiquidStakingToken {
    pub(crate) fn is_registered(&self, account_id: &AccountId) -> bool {
        self.storage_balance_of(account_id.clone())
            .is_some_and(|b| b.total >= FT_STORAGE_DEPOSIT)
    }
}

#[inline]
fn is_external_call() -> bool {
    env::current_account_id() != env::predecessor_account_id()
}
