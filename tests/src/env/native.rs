use near_api::types::tokens::UserBalance;

use crate::env::types::{Account, Contract};

pub trait Native {
    async fn near_balance(&self) -> anyhow::Result<UserBalance>;
}

impl Native for Account {
    async fn near_balance(&self) -> anyhow::Result<UserBalance> {
        self.inner
            .tokens()
            .near_balance()
            .fetch_from(self.config())
            .await
            .map_err(Into::into)
    }
}

impl Native for Contract {
    async fn near_balance(&self) -> anyhow::Result<UserBalance> {
        self.inner
            .as_account()
            .tokens()
            .near_balance()
            .fetch_from(self.config())
            .await
            .map_err(Into::into)
    }
}
