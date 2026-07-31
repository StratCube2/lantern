//! `InventoryClickEvent` / `InventoryCloseEvent` wrappers.
//!
//! `pumpkin-plugin-api` currently only hand-wraps `block`, `packet`, `player`,
//! and `server` events (see its `events/mod.rs`) — inventory events aren't
//! wrapped yet, even though `event.wit` defines `inventory-click-event-data`
//! and `inventory-close-event-data` and the host dispatches them. Since
//! `FromIntoEvent` is public API, we wrap these two ourselves, following the
//! exact pattern used by the crate's own `events/player/player_join.rs`.
//!
//! This is what the GUI kit-creator (`/lantern`) and kit-queue menu (`/gm`)
//! click handling is built on.

use pumpkin_plugin_api::events::FromIntoEvent;
use pumpkin_plugin_api::events_wit::{Event, EventType};

// Re-exported so callers only need `crate::inventory_events::*`.
pub use pumpkin_plugin_api::events_wit::{InventoryClickEventData, InventoryCloseEventData};

pub struct InventoryClickEvent;
impl FromIntoEvent for InventoryClickEvent {
    const EVENT_TYPE: EventType = EventType::InventoryClickEvent;
    type Data = InventoryClickEventData;

    fn data_from_event(event: Event) -> Self::Data {
        match event {
            Event::InventoryClickEvent(data) => data,
            _ => panic!("unexpected event"),
        }
    }

    fn data_into_event(data: Self::Data) -> Event {
        Event::InventoryClickEvent(data)
    }
}

pub struct InventoryCloseEvent;
impl FromIntoEvent for InventoryCloseEvent {
    const EVENT_TYPE: EventType = EventType::InventoryCloseEvent;
    type Data = InventoryCloseEventData;

    fn data_from_event(event: Event) -> Self::Data {
        match event {
            Event::InventoryCloseEvent(data) => data,
            _ => panic!("unexpected event"),
        }
    }

    fn data_into_event(data: Self::Data) -> Event {
        Event::InventoryCloseEvent(data)
    }
}
