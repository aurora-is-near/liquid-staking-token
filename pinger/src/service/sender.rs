use backon::{ExponentialBuilder, Retryable};
use serde_json::Value;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;

use crate::service::message::Message;

const MAX_NONCE_RETRIES: u32 = 5;
const PING_GAS: near_kit::Gas = near_kit::Gas::from_tgas(50);

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TxSenderConfig {
    #[serde(default)]
    pub rpc_url: Option<String>,
    pub account_id: String,
    pub private_key: String,
    pub contract_id: String,
    pub method_name: String,
    pub args: Option<Value>,
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
                near_kit::Near::custom(url, "mainnet")
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
                        let result = (|| self.send_ping_transaction())
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

    async fn send_ping_transaction(&self) -> anyhow::Result<()> {
        let result = self
            .client
            .call_with_options(
                &self.config.contract_id,
                &self.config.method_name,
                &self.config.args,
                PING_GAS,
                near_kit::NearToken::ZERO,
            )
            .await?;

        anyhow::ensure!(result.is_success(), "Ping transaction failed: {result:?}");

        Ok(())
    }
}
