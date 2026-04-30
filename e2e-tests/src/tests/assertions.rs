use near_api::NearToken;
use near_api::types::storage::StorageBalance;
use near_api::types::transaction::result::TransactionResultError;

/// Asserts that a transaction failed and its rendered `Failure` contains `expected`.
///
/// # Panics
/// Panics if the transaction succeeds, is pending, or fails with another message.
pub(super) fn assert_transaction_failure_contains<T>(result: anyhow::Result<T>, expected: &str) {
    let Err(error) = result else {
        panic!("Expected transaction to fail");
    };
    let tx_error = error
        .downcast_ref::<TransactionResultError>()
        .expect("Expected transaction result error");

    match tx_error {
        TransactionResultError::Failure(failure) => {
            let failure = failure.to_string();
            assert!(
                failure.contains(expected),
                "Expected transaction failure to contain `{expected}`, got `{failure}`"
            );
        }
        TransactionResultError::Pending(status) => {
            panic!("Expected transaction failure: {status:?}");
        }
    }
}

/// Asserts a registered storage balance and verifies `locked = total - available`.
///
/// # Panics
/// Panics if the account is unregistered or any storage balance field differs.
pub(super) fn assert_storage_balance(
    storage_balance: Option<StorageBalance>,
    total: NearToken,
    available: NearToken,
) {
    let storage_balance = storage_balance.expect("Expected account to be registered");
    assert_eq!(storage_balance.total, total);
    assert_eq!(storage_balance.available, available);
    assert_eq!(storage_balance.locked, total.saturating_sub(available));
}
