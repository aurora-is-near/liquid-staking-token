use crate::service::sender::TxSender;
use crate::service::watcher::EpochWatcher;

pub use config::Config;

mod config;
mod message;
mod sender;
mod watcher;

pub async fn run(config: Config) -> anyhow::Result<()> {
    let (sender, receiver) = tokio::sync::mpsc::channel(10);

    let sender_handle = TxSender::new(&config.sender, receiver)
        .map_err(|e| anyhow::anyhow!("Failed to create ping sender: {e}"))?
        .run();
    let watcher_handler = EpochWatcher::new(&config, sender)
        .map_err(|e| anyhow::anyhow!("Failed to create epoch watcher: {e}"))?
        .run();

    tokio::try_join!(watcher_handler, sender_handle)?;

    Ok(())
}
