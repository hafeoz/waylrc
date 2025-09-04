#![feature(btree_cursors)]
use std::time::Duration;

use anyhow::Result;
use clap::Parser as _;
use event_loop::event_loop;
use zbus::Connection;

mod args;
mod dbus;
mod event_loop;
mod external_lrc_provider;
mod lrc;
mod output;
mod player;
mod utils;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = args::Args::parse();
    args.init_tracing_subscriber();
    let filter_keys = args.skip_metadata.into_iter().collect();
    let allowed_players = args.player.clone();

    let connection = Connection::session().await?;
    event_loop(
        connection,
        Duration::from_secs_f64(args.refresh_every),
        filter_keys,
        allowed_players,
        args.external_lrc_provider,
        args.navidrome_server_url,
        args.navidrome_username,
        args.navidrome_password,
    )
    .await
}
