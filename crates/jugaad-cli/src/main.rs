use clap::{Parser, Subcommand};

#[derive(Debug,Parser)]
#[command(name="jugaad", version, about="Rust implication of jugaad-data i.e Indian market data downloader")]
struct Cli {
    #[command(subcommand)]
    command:Command,
}

#[derive(Debug,Subcommand)]
enum Command {
    Version,
}

#[tokio::main]

async fn main()->anyhow::Result<()> {
    tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
    .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Version=>println!("jugaad-core {}",jugaad_core::version()),
    }
    Ok(())
}

