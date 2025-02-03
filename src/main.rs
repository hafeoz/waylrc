use std::time::Duration;

use anyhow::Result;
use clap::Parser as _;
use event_loop::event_loop;
use zbus::Connection;

mod args;
mod dbus;
mod event_loop;
mod lrc;
mod output;
mod player;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = args::Args::parse();
    args.init_tracing_subscriber();
    let filter_keys = args.skip_metadata.into_iter().collect();

    let connection = Connection::session().await?;
    event_loop(
        connection,
        Duration::from_secs_f64(args.refresh_every),
        filter_keys,
    )
    .await
}
