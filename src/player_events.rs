//! Local wrappers for player action events used by the plugin, plus
//! connection-lifecycle handlers.
//!
//! `PlayerChatEvent`/`PlayerMoveEvent` — `pumpkin-plugin-api` does not
//! currently hand-wrap these events, but the WIT source exposes them, so we
//! bridge them here using the same pattern as the local inventory wrappers
//! (`inventory_events.rs`). `lobby.rs`'s deferred-teleport logic
//! (`TeleportPendingOnAction`) is built on top of these.
//!
//! `CleanupOnLeave` is the piece that satisfies "player leaves mid-match,
//! declare the other side the winner and finish the match": on
//! `PlayerLeaveEvent` it forwards to `MatchManager::handle_player_left`
//! (which handles forfeiting the leaver's team and resolving the match) in
//! addition to the pre-existing queue/state cleanup, so a disconnect is
//! handled correctly regardless of which of the three places
//! (lobby/queue/match) the player was in when they left.
//!
//! `SetStateOnJoin` marks a newly-joined player as `PlayerState::Lobby`.
//! Note this only touches player *state*, not position — the actual lobby
//! teleport is handled separately by `lobby.rs`'s `TeleportToLobbyOnJoin` /
//! `TeleportPendingOnAction` pair, which defer the teleport until the
//! player's first chat or move event after joining (rather than teleporting
//! immediately on `PlayerJoinEvent`, which can race the client still
//! finishing its own join/loading sequence). Both handlers register
//! independently for `PlayerJoinEvent` in `lib.rs`.

use std::sync::Arc;

use pumpkin_plugin_api::events::EventHandler;
use pumpkin_plugin_api::events::player::{PlayerJoinEvent, PlayerLeaveEvent};
use pumpkin_plugin_api::events_wit::{Event, EventType, PlayerJoinEventData, PlayerLeaveEventData};
use pumpkin_plugin_api::uuid;

pub use pumpkin_plugin_api::events_wit::{PlayerChatEventData, PlayerMoveEventData};
use pumpkin_plugin_api::events::FromIntoEvent;

use crate::lobby::LobbyStore;
use crate::matches::MatchManager;
use crate::open_menu::OpenMenuRegistry;
use crate::queue::QueueManager;
use crate::state::{PlayerState, StateRegistry};

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

pub struct CleanupOnLeave {
    pub queue: Arc<QueueManager>,
    pub state: Arc<StateRegistry>,
    pub matches: Arc<MatchManager>,
    pub open_menus: Arc<OpenMenuRegistry>,
    pub lobby: Arc<LobbyStore>,
}

impl EventHandler<PlayerLeaveEvent> for CleanupOnLeave {
    fn handle(
        &self,
        server: pumpkin_plugin_api::Server,
        data: PlayerLeaveEventData,
    ) -> PlayerLeaveEventData {
        let uuid_str = uuid::to_string(data.player.get_id());

        // If they were mid-match, this forfeits their team and immediately
        // declares the opposing side the winner (see
        // MatchManager::handle_player_left's doc comment) -- has to run
        // before the state/queue cleanup below so the match engine still
        // sees them as a live fighter when it looks them up by uuid.
        self.matches.handle_player_left(&server, &uuid_str);

        self.queue.dequeue_all(&uuid_str);
        self.state.remove(&uuid_str);
        self.open_menus.clear(&uuid_str);
        // Drop any pending lobby.rs deferred-teleport flag so a disconnect
        // mid-loading-screen doesn't leave a stale entry for a uuid that
        // will never send another chat/move event to consume it.
        self.lobby.clear_pending(&uuid_str);

        data
    }
}

/// Marks newly-joined players as being in the lobby state. Does not
/// teleport them -- see `lobby.rs` for the deferred teleport-on-first-action
/// handling, registered separately.
pub struct SetStateOnJoin {
    pub state: Arc<StateRegistry>,
}

impl EventHandler<PlayerJoinEvent> for SetStateOnJoin {
    fn handle(&self, _server: pumpkin_plugin_api::Server, data: PlayerJoinEventData) -> PlayerJoinEventData {
        let uuid_str = uuid::to_string(data.player.get_id());
        self.state.set(&uuid_str, PlayerState::Lobby);
        data
    }
}
