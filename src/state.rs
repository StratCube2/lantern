//! The Player State Machine.
//!
//! Every connected player is conceptually in exactly one of these states.
//! Every queue/command/GUI action must validate the caller's current state
//! before doing anything, which is what stops a player queueing twice,
//! queueing mid-fight, etc.
//!
//! NOTE on event coverage: an earlier pass assumed `PlayerLeaveEvent` wasn't
//! implemented yet (tracked against an old issue). That's no longer true —
//! `pumpkin-plugin-api`'s `events/player/player_leave.rs` wraps it, and
//! `lib.rs` now clears state on it directly. The periodic reconciliation
//! task is kept anyway as a safety net (covers crashes/disconnects that
//! might not cleanly fire the event, and any state left over from before
//! this fix was made).

use std::collections::HashMap;
use std::sync::RwLock;

pub type PlayerUuid = String; // TODO: verify real UUID type exposed by pumpkin-plugin-api (likely uuid::Uuid or a wrapper)

/// A queueable mode. The four original built-ins keep fixed variants (so
/// `/queue nodebuff` etc. don't depend on kit registry contents existing).
/// `Kit(name)` is added for `/gm`, where every mode is a runtime-created kit
/// rather than one of these fixed four — this is intentionally open-ended
/// since kits are created/deleted at runtime via `/lantern`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum QueueMode {
    NoDebuff1v1,
    Sumo1v1,
    Gapple1v1,
    Teams2v2,
    Kit(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerState {
    Lobby,
    Queued { mode: QueueMode },
    InMatch { match_id: u64 },
    Spectating { match_id: u64 },
}

/// Global, thread-safe registry of player states.
///
/// Pumpkin plugins run in an async, multi-threaded host, so this needs to be
/// safe to touch from command handlers, event handlers, and the scheduled
/// task concurrently. `std::sync::RwLock` is a placeholder — if the plugin
/// API's async runtime penalizes blocking locks, swap this for
/// `tokio::sync::RwLock` (the doc's own snippets use `tokio::sync::Mutex`
/// elsewhere, so tokio is almost certainly already in the dependency tree).
pub struct StateRegistry {
    inner: RwLock<HashMap<PlayerUuid, PlayerState>>,
}

impl StateRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    /// Returns the player's current state, defaulting to (and persisting) `Lobby`
    /// if this is the first time we've seen them.
    pub fn get_or_init(&self, uuid: &str) -> PlayerState {
        {
            let map = self.inner.read().unwrap();
            if let Some(state) = map.get(uuid) {
                return state.clone();
            }
        }
        let mut map = self.inner.write().unwrap();
        map.entry(uuid.to_string())
            .or_insert(PlayerState::Lobby)
            .clone()
    }

    pub fn set(&self, uuid: &str, state: PlayerState) {
        let mut map = self.inner.write().unwrap();
        map.insert(uuid.to_string(), state);
    }

    pub fn remove(&self, uuid: &str) {
        let mut map = self.inner.write().unwrap();
        map.remove(uuid);
    }

    /// Drop any tracked players who are not in `online_uuids`. Call this from
    /// the periodic task to work around the missing PlayerQuitEvent.
    pub fn reconcile(&self, online_uuids: &[String]) {
        let mut map = self.inner.write().unwrap();
        map.retain(|uuid, _| online_uuids.contains(uuid));
    }

    /// Convenience check used before letting a player queue.
    pub fn is_lobby(&self, uuid: &str) -> bool {
        matches!(self.get_or_init(uuid), PlayerState::Lobby)
    }
}

impl Default for StateRegistry {
    fn default() -> Self {
        Self::new()
    }
}
