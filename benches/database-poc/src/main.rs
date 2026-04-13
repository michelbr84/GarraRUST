//! GAR-373 benchmark harness: Postgres vs SQLite for garraia-workspace.
//!
//! Ephemeral — delete after ADR 0003 merges.

mod shared;
mod postgres_scenarios;
mod sqlite_scenarios;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "database-poc", version, about = "GAR-373 benchmark harness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run all Postgres scenarios (B1-B7) against an ephemeral testcontainer.
    Postgres {
        /// Emit JSON result to this path.
        #[arg(long, default_value = "results-postgres.json")]
        out: String,
        /// Run N iterations per scenario and report the median.
        #[arg(long, default_value_t = 3)]
        iterations: u32,
    },
    /// Run all SQLite scenarios (B1-B4; B5-B7 are N/A) against a tempfile.
    Sqlite {
        #[arg(long, default_value = "results-sqlite.json")]
        out: String,
        #[arg(long, default_value_t = 3)]
        iterations: u32,
    },
    /// Run both backends and emit a merged JSON.
    All {
        #[arg(long, default_value = "results-merged.json")]
        out: String,
        #[arg(long, default_value_t = 3)]
        iterations: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Postgres { out, iterations } => {
            let results = postgres_scenarios::run_all(iterations).await?;
            shared::write_results(&out, &results)?;
            tracing::info!("postgres results written to {out}");
        }
        Command::Sqlite { out, iterations } => {
            let results = sqlite_scenarios::run_all(iterations)?;
            shared::write_results(&out, &results)?;
            tracing::info!("sqlite results written to {out}");
        }
        Command::All { out, iterations } => {
            let pg = postgres_scenarios::run_all(iterations).await?;
            let sq = sqlite_scenarios::run_all(iterations)?;
            let merged = shared::merge_results(pg, sq);
            shared::write_results(&out, &merged)?;
            tracing::info!("merged results written to {out}");
        }
    }
    Ok(())
}
