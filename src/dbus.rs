use anyhow::{Context as _, Result};
use futures_lite::{stream::iter, Stream, StreamExt as _};
use zbus::{fdo::DBusProxy, names::OwnedBusName, Connection};

pub mod media_player2;
pub mod player;
pub mod playlists;
pub mod track_list;

pub enum BusActivity {
    Created,
    Destroyed,
}

/// D-Bus's activity parsed from `NameOwnerChanged` signal
pub struct BusChange {
    pub name: OwnedBusName,
    pub activity: BusActivity,
}
impl BusChange {
    pub const fn new(name: OwnedBusName, activity: BusActivity) -> Self {
        Self { name, activity }
    }
    pub const fn new_existing(name: OwnedBusName) -> Self {
        Self {
            name,
            activity: BusActivity::Created,
        }
    }
    pub fn is_mpris(&self) -> bool {
        self.name.starts_with("org.mpris.MediaPlayer2")
    }
}

/// Return a stream of all MPRIS players on the bus
pub async fn player_buses(conn: &Connection) -> Result<impl Stream<Item = BusChange>> {
    let proxy = DBusProxy::new(conn)
        .await
        .context("Failed to create DBusProxy")?;

    let existing_names = iter(
        proxy
            .list_names()
            .await
            .context("Failed to list currently-owned names on DBus")?
            .into_iter()
            .map(BusChange::new_existing),
    );
    let new_activities = proxy
        .receive_name_owner_changed()
        .await
        .context("Failed to listen for NameOwnerChanged signal on DBus")?
        .filter_map(|s| {
            let args = s
                .args()
                .inspect_err(|e| tracing::warn!(?e, "Failed to parse NameOwnerChanged argument"))
                .ok()?;
            let change = match (args.new_owner.is_some(), args.old_owner.is_some()) {
                (true, false) => BusActivity::Created,
                (false, true) => BusActivity::Destroyed,
                _ => return None,
            };
            Some(BusChange::new(args.name.into(), change))
        });

    Ok(existing_names
        .chain(new_activities)
        .filter(BusChange::is_mpris))
}
