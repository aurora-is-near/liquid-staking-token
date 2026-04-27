use near_sdk::serde_json::json;
use testresult::TestResult;

use crate::env::Env;
use crate::env::pool::StakingPool;

#[tokio::test]
async fn test_set_protocol_fee_bps() -> TestResult {
    let env = Env::builder().build().await?;

    assert_eq!(
        env.lst.get_reward_fee_fraction().await?,
        json!({"numerator":0, "denominator":10000})
    );

    // Attempt to set protocol fee as non-owner should fail.
    let result = env.intents.set_protocol_fee_bps(1_000).await;
    assert!(result.is_err(), "Expected setting fee as non-owner to fail");

    // Owner can set protocol fee.
    env.lst.set_protocol_fee_bps(1_000).await?;

    assert_eq!(
        env.lst.get_reward_fee_fraction().await?,
        json!({"numerator":1000, "denominator":10000})
    );

    Ok(())
}
