use near_api::types::transaction::result::TransactionResult;
use testresult::TestResult;

/// Asserts that two full transaction results succeeded and were included in the same block.
pub(super) fn assert_same_block_success(
    result1: TransactionResult,
    result2: TransactionResult,
    pending_message1: &str,
    pending_message2: &str,
) -> TestResult {
    let result1 = result1
        .into_full()
        .ok_or_else(|| anyhow::anyhow!("{pending_message1}"))?;
    let result2 = result2
        .into_full()
        .ok_or_else(|| anyhow::anyhow!("{pending_message2}"))?;

    assert_eq!(&result1.outcome().block_hash, &result2.outcome().block_hash);

    result1.into_result()?;
    result2.into_result()?;

    Ok(())
}
