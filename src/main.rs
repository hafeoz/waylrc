use std::time::Duration;

use anyhow::Result;
use clap::Parser as _;
use event_loop::event_loop;
use zbus::Connection;

mod dbus;
mod player;
mod lrc;
mod event_loop;
mod output;
mod args;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = args::Args::parse();
    args.init_tracing_subscriber();

    let connection = Connection::session().await?;
    event_loop(connection, Duration::from_secs_f64(args.refresh_every)).await
}
