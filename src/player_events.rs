//! Local wrappers for player action events used by the plugin.
//!
//! `pumpkin-plugin-api` does not currently hand-wrap these events, but the
//! WIT source exposes them, so we bridge them here using the same pattern as
//! the local inventory wrappers.

use pumpkin_plugin_api::events::FromIntoEvent;
use pumpkin_plugin_api::events_wit::{Event, EventType};

pub use pumpkin_plugin_api::events_wit::{PlayerChatEventData, PlayerMoveEventData};

pub struct PlayerChatEvent;
impl FromIntoEvent for PlayerChatEvent {
    const EVENT_TYPE: EventType = EventType::PlayerChatEvent;
    type Data = PlayerChatEventData;

    fn data_from_event(event: Event) -> Self::Data {
        match event {
            Event::PlayerChatEvent(data) => data,
            _ => panic!("unexpected event"),
        }
    }

    fn data_into_event(data: Self::Data) -> Event {
        Event::PlayerChatEvent(data)
    }
}

pub struct PlayerMoveEvent;
impl FromIntoEvent for PlayerMoveEvent {
    const EVENT_TYPE: EventType = EventType::PlayerMoveEvent;
    type Data = PlayerMoveEventData;

    fn data_from_event(event: Event) -> Self::Data {
        match event {
            Event::PlayerMoveEvent(data) => data,
            _ => panic!("unexpected event"),
        }
    }

    fn data_into_event(data: Self::Data) -> Event {
        Event::PlayerMoveEvent(data)
    }
}
