use std::path::{Path, PathBuf};

use crate::service::sender::TxSenderConfig;

/// Service config
#[derive(Debug, serde::Deserialize)]
pub struct Config {
    /// Block client config.
    pub client: block_client_rs::Config,
    /// Transaction sender config.
    pub sender: TxSenderConfig,
    /// Log level.
    pub log_level: String,
    /// Path where to save epoch id.
    pub epoch_id_path: PathBuf,
}

impl Config {
    pub fn parse<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        std::fs::File::open(path)
            .map(std::io::BufReader::new)
            .and_then(|reader| {
                serde_yaml::from_reader(reader).map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Failed to parse config: {e}"),
                    )
                })
            })
            .map_err(Into::into)
    }
}
