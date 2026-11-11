//! The answer log from the UI's side: [`yomeru_core::drill_log`] with the
//! platform taken from Dioxus context.

use dioxus::prelude::consume_context;

pub use yomeru_core::drill_log::*;

use crate::platform::Platform;
use crate::types::SrsCard;

/// See [`yomeru_core::drill_log::record_drill`].
pub fn record_drill(card: &SrsCard, rating: u8) {
    yomeru_core::drill_log::record_drill(&consume_context::<Platform>(), card, rating);
}
