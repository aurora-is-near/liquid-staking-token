use near_api::Signer;
use std::sync::Arc;

pub struct Account {
    pub inner: near_api::Account,
    config: near_api::NetworkConfig,
    signer: Arc<Signer>,
}

impl Account {
    pub fn new(
        account: near_api::Account,
        config: near_api::NetworkConfig,
        signer: Arc<Signer>,
    ) -> Self {
        Self {
            inner: account,
            config,
            signer,
        }
    }

    pub fn id(&self) -> &near_api::AccountId {
        self.inner.account_id()
    }

    pub fn config(&self) -> &near_api::NetworkConfig {
        &self.config
    }

    pub fn signer(&self) -> Arc<Signer> {
        Arc::clone(&self.signer)
    }
}

pub struct Contract {
    pub inner: near_api::Contract,
    config: near_api::NetworkConfig,
    signer: Arc<Signer>,
}

impl Contract {
    pub fn new(
        contract: near_api::Contract,
        config: near_api::NetworkConfig,
        signer: Arc<Signer>,
    ) -> Self {
        Self {
            inner: contract,
            config,
            signer,
        }
    }

    pub fn id(&self) -> &near_api::AccountId {
        self.inner.account_id()
    }

    pub fn config(&self) -> &near_api::NetworkConfig {
        &self.config
    }

    pub fn signer(&self) -> Arc<Signer> {
        Arc::clone(&self.signer)
    }
}
