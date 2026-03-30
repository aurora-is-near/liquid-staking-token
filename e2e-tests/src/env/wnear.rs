use near_api::NearToken;
use near_sdk::serde_json::json;

use crate::env::ft::FungibleToken;
use crate::env::types::{Account, Contract};

pub trait WNear: FungibleToken {
    async fn near_deposit(&self, account: &Account, amount: NearToken) -> anyhow::Result<()>;
    async fn near_withdraw(&self, account: &Account, amount: NearToken) -> anyhow::Result<()>;
}

impl WNear for Contract {
    async fn near_deposit(&self, account: &Account, amount: NearToken) -> anyhow::Result<()> {
        self.inner
            .call_function("near_deposit", ())
            .transaction()
            .deposit(amount)
            .with_signer(account.id().clone(), account.signer())
            .send_to(self.config())
            .await?
            .assert_success();

        Ok(())
    }

    async fn near_withdraw(&self, account: &Account, amount: NearToken) -> anyhow::Result<()> {
        self.inner
            .call_function(
                "near_withdraw",
                json!({
                    "amount": amount,
                }),
            )
            .transaction()
            .deposit(NearToken::from_yoctonear(1))
            .with_signer(account.id().clone(), account.signer())
            .send_to(self.config())
            .await?
            .assert_success();

        Ok(())
    }
}
