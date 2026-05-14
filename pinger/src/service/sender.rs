use backon::{ExponentialBuilder, Retryable};
use near_kit::Gas;
use serde_json::Value;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;

use crate::service::message::Message;

const MAX_NONCE_RETRIES: u32 = 5;
const PING_GAS: Gas = Gas::from_tgas(50);
const DEFAULT_CHAIN_ID: &str = "mainnet";

/// Transaction sender configuration.
#[derive(Clone, serde::Deserialize)]
pub struct TxSenderConfig {
    /// NEAR node RPC url.
    pub rpc_url: Option<String>,
    /// NEAR chain id.
    pub chain_id: Option<String>,
    /// Account id of the signer of the transaction.
    pub account_id: String,
    /// Private key of the signer of the transaction.
    pub private_key: String,
    /// Account id of the contract to call.
    pub contract_id: String,
    /// Method name to call.
    pub method_name: String,
    /// Arguments to call the method with.
    pub args: Option<Value>,
    /// Amount of gas to pay for the transaction.
    pub gas: Option<Gas>,
}

pub struct TxSender {
    client: near_kit::Near,
    config: TxSenderConfig,
    receiver: Receiver<Message>,
}

impl TxSender {
    pub fn new(config: &TxSenderConfig, receiver: Receiver<Message>) -> anyhow::Result<Self> {
        let private_key =
            std::env::var("PRIVATE_KEY").unwrap_or_else(|_| config.private_key.clone());
        let client = config
            .rpc_url
            .as_ref()
            .map_or_else(near_kit::Near::mainnet, |url| {
                near_kit::Near::custom(url, config.chain_id.as_deref().unwrap_or(DEFAULT_CHAIN_ID))
            })
            .max_nonce_retries(MAX_NONCE_RETRIES)
            .credentials(private_key, &config.account_id)?
            .build();

        Ok(Self {
            client,
            config: config.clone(),
            receiver,
        })
    }

    pub fn run(mut self) -> JoinHandle<()> {
        tracing::debug!("tx sender started");
        tokio::spawn(async move {
            let back_off = ExponentialBuilder::default().with_max_times(5);

            while let Some(msg) = self.receiver.recv().await {
                match msg {
                    Message::EpochChanged => {
                        tracing::info!("epoch has been changed, sending ping...");
                        let result = (|| self.send_transaction())
                            .retry(back_off)
                            .notify(|err, dur| {
                                tracing::error!("{err}, retrying after {} sec", dur.as_secs());
                            })
                            .await;

                        if let Err(e) = result {
                            tracing::error!("failed to send ping transaction: {e}");
                        }
                    }
                    Message::Shutdown => {
                        tracing::info!("shutdown signal received, stopping sender...");
                        break;
                    }
                }
            }
        })
    }

    async fn send_transaction(&self) -> anyhow::Result<()> {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.client.call_with_options(
                &self.config.contract_id,
                &self.config.method_name,
                &self.config.args,
                self.config.gas.unwrap_or(PING_GAS),
                near_kit::NearToken::ZERO,
            ),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Transaction timed out"))?
        .map_err(|_| anyhow::anyhow!("Failed to send transaction"))?;

        anyhow::ensure!(result.is_success(), "Transaction failed: {result:?}");

        Ok(())
    }
}
