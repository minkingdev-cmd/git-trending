mod collect;
mod graphql;
mod search;
mod store;
mod trending;

use clap::Parser;
use collect::Collector;
use ght_core::config::{collect_time_parts, Settings};
use ght_core::db;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "ght-collector", about = "GitHub leaderboard collector")]
struct Cli {
    /// Run a single collection and exit (for CronJob / manual runs)
    #[arg(long)]
    once: bool,
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("gh-trending-collector/0.1")
        .build()
        .expect("failed to build http client")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();
    let settings = Settings::from_env()?;
    let pool = db::pg_pool(&settings.database_url).await?;
    db::migrate(&pool).await?;

    let collector = Arc::new(Collector {
        pool,
        http: http_client(),
        settings: settings.clone(),
        github_base: "https://github.com".to_string(),
        api_base: "https://api.github.com".to_string(),
    });

    if cli.once {
        let report = collector.collect_once().await;
        tracing::info!(ok = report.ok, failed = report.failed, "collection finished");
        anyhow::ensure!(report.ok > 0, "all collection tasks failed");
        return Ok(());
    }

    let (hour, minute) = collect_time_parts(&settings.collect_time)?;
    let cron = format!("0 {minute} {hour} * * *");
    let scheduler = JobScheduler::new().await?;
    let ctx = collector.clone();
    scheduler
        .add(Job::new_async(cron.as_str(), move |_uuid, _lock| {
            let ctx = ctx.clone();
            Box::pin(async move {
                let report = ctx.collect_once().await;
                tracing::info!(ok = report.ok, failed = report.failed, "scheduled collection finished");
            })
        })?)
        .await?;
    scheduler.start().await?;
    tracing::info!(collect_time = %settings.collect_time, "collector daemon started");
    tokio::signal::ctrl_c().await?;
    tracing::info!("collector shutting down");
    Ok(())
}
