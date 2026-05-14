use block_client_rs::stream::read::{BlocksStream, ReadStream};
use block_client_rs::types::bus_message::payloads::near_block::NEARBlock;
use block_client_rs::types::bus_message::BusMessage;
use block_client_rs::types::request::{BlocksRequestBuilder, DeliverySettings, StartPolicy};
use block_client_rs::types::BlockMessage;
use block_client_rs::BlockClient;
use near_kit::CryptoHash;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use tokio::select;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::service::message::Message;
use crate::service::Config;

const TIMEOUT: Duration = Duration::from_secs(20);
const RECREATE_STREAM_DELAY: Duration = Duration::from_secs(10);

pub struct EpochWatcher {
    client: BlockClient,
    stream_name: String,
    sender: Sender<Message>,
    epoch_id: Option<CryptoHash>,
    epoch_id_path: PathBuf,
}

impl EpochWatcher {
    pub fn new(config: &Config, sender: Sender<Message>) -> anyhow::Result<Self> {
        Ok(Self {
            client: BlockClient::new(config.client.clone())?,
            stream_name: config.client.stream_name.clone(),
            sender,
            epoch_id: load_epoch_id(&config.epoch_id_path),
            epoch_id_path: config.epoch_id_path.clone(),
        })
    }

    pub fn run(mut self) -> JoinHandle<()> {
        tracing::info!("epoch watcher started");
        tokio::spawn(async move {
            let ctrl_c = tokio::signal::ctrl_c();
            tokio::pin!(ctrl_c);

            let mut stream = self
                .create_stream()
                .await
                .inspect_err(|e| tracing::error!("Failed to create a stream: {e}"))
                .unwrap();

            loop {
                select! {
                    block = timeout(TIMEOUT, stream.next()) => {
                        if let Ok(Ok(msg)) = block {
                             if let Ok(current_epoch_id) = current_epoch_id(&msg) {
                                 tracing::debug!("received block from epoch id: {current_epoch_id}");

                                 if self.epoch_id.is_none_or(|id| id != current_epoch_id) {
                                     tracing::info!("epoch change detected: {:?} -> {}", self.epoch_id, current_epoch_id);

                                    let _ = self.sender.send(Message::EpochChanged).await;
                                     self.epoch_id = Some(current_epoch_id);
                                     self.save_epoch_id(current_epoch_id).await;

                                 }
                             } else {
                                 tracing::error!("bad block received");
                             }

                         } else {
                             tracing::error!("error receiving block, recreating stream...");
                             loop {
                                 match self.create_stream().await {
                                     Ok(s) => {
                                         stream = s;
                                         break;
                                     }
                                     Err(err) => {
                                          tracing::error!("{err}, retrying after {} sec", RECREATE_STREAM_DELAY.as_secs());
                                          tokio::time::sleep(RECREATE_STREAM_DELAY).await;
                                      }
                                 }
                             }
                         }
                    }
                    _ = &mut ctrl_c => {
                        tracing::info!("received ctrl-c signal, stopping watcher...");
                        if let Some(epoch_id) = self.epoch_id {
                            self.save_epoch_id(epoch_id).await;
                        }

                        let _ = self.sender.send(Message::Shutdown).await;

                        break;
                    }
                }
            }
        })
    }

    async fn create_stream(&mut self) -> anyhow::Result<BlocksStream> {
        let request = BlocksRequestBuilder::new()
            .with_stream_name(&self.stream_name)
            .with_start_policy(StartPolicy::StartOnLatestAvailable)
            .with_delivery_settings(DeliverySettings {
                exclude_payload: false,
                allow_compression: 1,
            })
            .build();

        self.client.get_block_stream(request).await
    }

    async fn save_epoch_id(&self, epoch_id: CryptoHash) {
        let _ = tokio::fs::write(&self.epoch_id_path, epoch_id.to_string())
            .await
            .inspect_err(|e| tracing::error!("Failed to save epoch id: {e}"));
    }
}

fn current_epoch_id(msg: &BlockMessage) -> anyhow::Result<CryptoHash> {
    BusMessage::<NEARBlock>::deserialize(&msg.payload)
        .map(|msg| CryptoHash::from_bytes(msg.payload.block.header.epoch_id.0))
        .map_err(Into::into)
}

fn load_epoch_id<P: AsRef<Path>>(path: P) -> Option<CryptoHash> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|id| CryptoHash::from_str(&id).ok())
}
