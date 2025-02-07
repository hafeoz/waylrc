use std::{sync::Arc, time::Duration};

use anyhow::{bail, ensure, Result};
use tokio::{
    sync::mpsc,
    task::{spawn, JoinHandle},
};
use tracing::instrument;
use zbus::{names::OwnedBusName, Connection};

use crate::{
    dbus::player::PlayerProxy,
    player::{PlayerInformation, PlayerInformationUpdate, PlayerInformationUpdateListener},
};

#[instrument(skip_all)]
async fn build_player<'a>(
    player_name: &Arc<OwnedBusName>,
    conn: Connection,
) -> Result<PlayerProxy<'a>> {
    let mut timeout = Duration::from_secs(2);
    loop {
        let player = PlayerProxy::builder(&conn)
            .destination(Arc::unwrap_or_clone(Arc::clone(player_name)))?
            .path("/org/mpris/MediaPlayer2")?
            .build()
            .await?;

        // Probe the connection - sometimes the newly created player is not ready for messages yet,
        // and we need to restart the proxy
        if tokio::time::timeout(timeout, async {
            let _ = player.can_play().await;
        })
        .await
        .is_ok()
        {
            return Ok(player);
        }

        // Increase the timeout exponentially
        if timeout.as_secs() > 10 {
            bail!("PlayerProxy is not responding after {timeout:#?} - giving up!");
        }
        tracing::info!("PlayerProxy is not responding after {timeout:#?} - restarting connection");
        timeout = Duration::from_secs_f64(1.3 * timeout.as_secs_f64());
    }
}

#[instrument(skip_all, fields(player_name))]
pub async fn get_player_info(
    player_name: Arc<OwnedBusName>,
    conn: Connection,
    refresh_interval: Duration,
    update_sender: mpsc::Sender<(Arc<OwnedBusName>, PlayerInformationUpdate)>,
) -> Result<(PlayerInformation, JoinHandle<Result<()>>)> {
    let player = build_player(&player_name, conn).await?;
    let info = PlayerInformation::new(&player).await?;
    tracing::debug!(?info);
    let mut info_updater = PlayerInformationUpdateListener::new(player, refresh_interval).await?;

    let info_updater_thread = spawn(async move {
        loop {
            let update = match info_updater.update().await {
                Ok(u) => u,
                Err(e) => {
                    tracing::warn!(?e, "Failed to parse MPRIS update");
                    continue;
                }
            };
            let result = update_sender.send((Arc::clone(&player_name), update)).await;
            ensure!(result.is_ok(), "Player updates listener closed");
        }
    });

    Ok((info, info_updater_thread))
}
