use crate::ONE_YOCTO;
use crate::test_utils::{context, id, pool};
use near_contract_standards::storage_management::StorageManagement;
use near_sdk::NearToken;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn pool_and_treasury_cannot_unregister_even_with_zero_balance() {
    let mut pool = pool();
    pool.internal_withdraw(&id("treasury.near"), NearToken::from_near(10));
    for account in ["pool.near", "treasury.near"] {
        context(account, 300, ONE_YOCTO, vec![], 1);
        assert!(catch_unwind(AssertUnwindSafe(|| pool.storage_unregister(Some(true)))).is_err());
        assert!(pool.storage_balance_of(id(account)).is_some());
    }
}
