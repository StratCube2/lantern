//! Thin `FromIntoEvent` wrappers for the two inventory events Lantern needs
//! (`InventoryClickEvent`/`InventoryCloseEvent`), following the exact same
//! pattern the host crate itself uses for e.g. `PlayerLeaveEvent`
//! (`pumpkin_plugin_api::events::player::player_leave`). These two aren't
//! pre-wrapped in `pumpkin_plugin_api::events` (only the `player`/`block`/
//! `server`/`packet` groups are), so `menu_router.rs` needs its own local
//! wrapper to hook `Context::register_event_handler`.

use pumpkin_plugin_api::EventType;
use pumpkin_plugin_api::events::{Event, FromIntoEvent};
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
