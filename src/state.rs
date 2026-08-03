//! Per-player state machine.
//!
//! Every online player is, at any moment, in exactly one of: the lobby, a
//! queue (waiting for a match to fill), an active match, or spectating one.
//! This is the single source of truth other modules gate on — `/gm`,
//! `/queue`, `/lantern <n>` (kit editing) all check `PlayerState::Lobby`
//! before letting a player start something new, and `MatchManager` flips
//! fighters to `InMatch`/back to `Lobby` as matches start and end.
//!
//! Deliberately in-memory only (not persisted to disk): a state like
//! `InMatch { match_id }` or `Queued { mode }` refers to live, in-process
//! bookkeeping (an active `RunningMatch`, a live queue pool) that doesn't
//! survive a server restart anyway, so there is nothing meaningful to
//! reload — every player just starts back at `Lobby` (via `get_or_init`'s
//! default) after a restart.

use std::collections::HashMap;
use std::sync::RwLock;

/// The matchmaking pool a player is queued in. The three fixed 1v1 modes
/// are placeholders for potential built-in game modes; in practice every
/// kit created via `/lantern` queues under `QueueMode::Kit(name)` — see
/// `queue_cmd.rs`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum QueueMode {
    Kit(String),
    NoDebuff1v1,
    Sumo1v1,
    Gapple1v1,
    Teams2v2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerState {
    /// Idle in the hub, free to queue, browse `/gm`, or edit kits.
    Lobby,
    /// Waiting in a `QueueManager` pool for a match to fill.
    Queued { mode: QueueMode },
    /// Actively fighting in a running match.
    InMatch { match_id: u64 },
    /// Eliminated from (or otherwise watching) a match that's still
    /// running, without being a live fighter in it.
    Spectating { match_id: u64 },
}

pub struct StateRegistry {
    inner: RwLock<HashMap<String, PlayerState>>,
}

impl StateRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    /// Returns this player's current state, defaulting to (and persisting)
    /// `Lobby` if they have no entry yet — e.g. a player who just joined.
    pub fn get_or_init(&self, uuid: &str) -> PlayerState {
        if let Some(state) = self.inner.read().unwrap().get(uuid) {
            return state.clone();
        }
        let mut map = self.inner.write().unwrap();
        map.entry(uuid.to_string()).or_insert(PlayerState::Lobby).clone()
    }

    pub fn get(&self, uuid: &str) -> Option<PlayerState> {
        self.inner.read().unwrap().get(uuid).cloned()
    }

    pub fn set(&self, uuid: &str, state: PlayerState) {
        self.inner.write().unwrap().insert(uuid.to_string(), state);
    }

    /// Drops this player's entry entirely (used on disconnect — there's no
    /// value in remembering a state for someone who isn't online, and it
    /// keeps the map from growing unboundedly across a long server
    /// uptime).
    pub fn remove(&self, uuid: &str) {
        self.inner.write().unwrap().remove(uuid);
    }
}

impl Default for StateRegistry {
    fn default() -> Self {
        Self::new()
    }
}
