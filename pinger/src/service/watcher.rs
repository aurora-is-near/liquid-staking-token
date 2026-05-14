use block_client_rs::BlockClient;
use block_client_rs::stream::read::{BlocksStream, ReadStream};
use block_client_rs::types::BlockMessage;
use block_client_rs::types::bus_message::BusMessage;
use block_client_rs::types::bus_message::payloads::near_block::NEARBlock;
use block_client_rs::types::request::{BlocksRequestBuilder, DeliverySettings, StartPolicy};
use near_kit::CryptoHash;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use tokio::select;
#[cfg(unix)]
use tokio::signal::unix::SignalKind;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::service::Config;
use crate::service::message::Message;

const TIMEOUT: Duration = Duration::from_secs(20);
const RECREATE_STREAM_DELAY: Duration = Duration::from_secs(10);

enum Event {
    Block(BlockMessage),
    StreamError,
    Reconnected,
    ReconnectFailed,
}

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
        #[cfg(unix)]
        let mut sigterm =
            tokio::signal::unix::signal(SignalKind::terminate()).expect("install SIGTERM handler");

        tokio::spawn(async move {
            let shutdown = async {
                #[cfg(unix)]
                {
                    select! {
                        _ = tokio::signal::ctrl_c() => {}
                        _ = sigterm.recv() => {}
                    }
                }
                #[cfg(not(unix))]
                {
                    let _ = tokio::signal::ctrl_c().await;
                }
            };
            tokio::pin!(shutdown);

            let mut stream: Option<BlocksStream> = None;

            loop {
                let event = select! {
                    event = self.next_event(&mut stream) => event,
                    () = &mut shutdown => {
                        tracing::info!("received ctrl-c or SIGTERM signal, stopping watcher...");
                        break;
                    }
                };
                self.handle_event(event).await;
            }

            if let Some(epoch_id) = self.epoch_id {
                self.save_epoch_id(epoch_id).await;
            }

            if self.sender.send(Message::Shutdown).await.is_err() {
                tracing::warn!("failed to send shutdown message; receiver already dropped");
            }
        })
    }

    async fn next_event(&mut self, stream: &mut Option<BlocksStream>) -> Event {
        match stream {
            Some(s) => {
                if let Ok(Ok(msg)) = timeout(TIMEOUT, s.next()).await {
                    Event::Block(msg)
                } else {
                    *stream = None;
                    Event::StreamError
                }
            }
            None => match self.create_stream().await {
                Ok(s) => {
                    *stream = Some(s);
                    Event::Reconnected
                }
                Err(err) => {
                    tracing::error!(
                        "failed to recreate stream: {err}, retrying after {} sec",
                        RECREATE_STREAM_DELAY.as_secs()
                    );
                    tokio::time::sleep(RECREATE_STREAM_DELAY).await;
                    Event::ReconnectFailed
                }
            },
        }
    }

    #[allow(clippy::cognitive_complexity)]
    async fn handle_event(&mut self, event: Event) {
        match event {
            Event::Block(msg) => {
                let current_epoch_id = match current_epoch_id(&msg) {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::error!("bad block received: {e}");
                        return;
                    }
                };

                tracing::debug!("received block from epoch id: {current_epoch_id}");

                if self.epoch_id.is_none_or(|id| id != current_epoch_id) {
                    tracing::info!(
                        "epoch change detected: {:?} -> {current_epoch_id}",
                        self.epoch_id
                    );

                    if self.sender.send(Message::EpochChanged).await.is_err() {
                        tracing::error!(
                            "failed to send epoch change notification; receiver dropped"
                        );
                    }

                    self.epoch_id = Some(current_epoch_id);
                    self.save_epoch_id(current_epoch_id).await;
                }
            }
            Event::StreamError => {
                tracing::error!("error receiving block, will reconnect");
            }
            Event::Reconnected => {
                tracing::info!("stream reconnected");
            }
            Event::ReconnectFailed => {}
        }
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
