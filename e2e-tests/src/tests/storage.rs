use testresult::TestResult;

use crate::env::Env;
use crate::env::ft::FungibleToken;
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::tests::ZERO_AMOUNT;

#[tokio::test]
async fn test_check_total_balance_after_storage_deposits() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    let balance_checker = async |contract: &crate::env::types::Contract| -> TestResult {
        assert_eq!(
            contract
                .near_balance()
                .await?
                .total
                .saturating_add(contract.near_balance().await?.locked),
            contract.get_total_balance().await?
        );

        Ok(())
    };

    env.lst.ft_storage_deposit(alice, alice.id()).await?;
    balance_checker(&env.lst).await?;

    env.lst
        .ft_storage_withdraw(alice, Some(ZERO_AMOUNT))
        .await?;
    balance_checker(&env.lst).await?;

    env.lst.ft_storage_withdraw(alice, None).await?;
    balance_checker(&env.lst).await?;

    env.lst.ft_storage_unregister(alice, None).await?;
    balance_checker(&env.lst).await?;

    env.lst.ft_storage_unregister(bob, Some(true)).await?;
    balance_checker(&env.lst).await?;

    Ok(())
}
