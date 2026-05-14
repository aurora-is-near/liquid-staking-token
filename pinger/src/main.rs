use clap::Parser;

mod logger;
mod service;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(short, long)]
    config: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::try_parse()?;
    let config = service::Config::parse(&args.config)?;

    logger::setup(&config.log_level)?;
    service::run(config).await?;

    Ok(())
}
