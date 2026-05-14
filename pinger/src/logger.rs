use tracing_subscriber::{EnvFilter, FmtSubscriber};

pub fn setup(tracing_level: &str) -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| {
            EnvFilter::default().add_directive(
                format!("{}={}", env!("CARGO_CRATE_NAME"), tracing_level)
                    .parse()
                    .unwrap(),
            )
        }))
        .finish();

    tracing::subscriber::set_global_default(subscriber).map_err(Into::into)
}
