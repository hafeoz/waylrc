use std::{sync::Arc, time::Duration};

use anyhow::{ensure, Result};
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

#[instrument(skip_all, fields(player_name))]
pub async fn get_player_info(
    player_name: Arc<OwnedBusName>,
    conn: Connection,
    refresh_interval: Duration,
    update_sender: mpsc::Sender<(Arc<OwnedBusName>, PlayerInformationUpdate)>,
) -> Result<(PlayerInformation, JoinHandle<Result<()>>)> {
    let player = PlayerProxy::builder(&conn)
        .destination(Arc::unwrap_or_clone(Arc::clone(&player_name)))?
        .path("/org/mpris/MediaPlayer2")?
        .build()
        .await?;
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
