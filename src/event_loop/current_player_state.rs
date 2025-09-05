use crate::lrc::{Lrc, TimeTag};
use std::sync::Arc;
use zbus::names::OwnedBusName;

pub struct CurrentPlayerState {
    pub bus: Arc<OwnedBusName>,
    pub lrc: Lrc,
    pub next_lrc_timetag: TimeTag,
}
