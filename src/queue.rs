//! Matchmaking queue pools.
//!
//! The number of players required to fire a match is no longer hardcoded
//! per `QueueMode` — the four fixed built-in modes (nodebuff/sumo/gapple are
//! 1v1, teams is 2v2) still have a sensible default via
//! `QueueMode::default_required_players()`, but `Kit(_)` modes pull their
//! real player count from `Kit::required_players()` (see kits.rs), which
//! respects the `/lantern kit "<name>" multiplayer` + team-size setting.
//! Callers pass the resolved count into `enqueue` directly.

use crate::state::QueueMode;
use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;

pub struct QueueManager {
    pools: RwLock<HashMap<QueueMode, VecDeque<String>>>, // uuid queue per mode
}

impl QueueMode {
    /// Fallback player count for modes where the caller doesn't have a more
    /// precise number on hand (e.g. the four fixed built-in modes). `Kit(_)`
    /// callers should prefer `Kit::required_players()` from the actual kit
    /// definition and pass that into `QueueManager::enqueue` instead of
    /// relying on this default.
    pub fn default_required_players(&self) -> usize {
        match self {
            QueueMode::NoDebuff1v1 | QueueMode::Sumo1v1 | QueueMode::Gapple1v1 => 2,
            QueueMode::Teams2v2 => 4,
            QueueMode::Kit(_) => 2,
        }
    }
}

pub enum EnqueueResult {
    /// Not enough players yet; still waiting. Carries the current pool size
    /// and how many are needed, for queue-position/action-bar feedback.
    Waiting { in_queue: usize, needed: usize },
    /// Enough players were pooled — here are the uuids to pull into a match,
    /// already popped off the queue.
    MatchReady(Vec<String>),
}

impl QueueManager {
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
        }
    }

    /// Enqueues `uuid` under `mode`, firing a match once `needed` players
    /// have accumulated. `needed` is resolved by the caller (typically
    /// `Kit::required_players()`) rather than being fixed per-mode, so
    /// multiplayer/XvX kits work without a special `QueueMode` variant.
    pub fn enqueue(&self, mode: QueueMode, uuid: String, needed: usize) -> EnqueueResult {
        let mut pools = self.pools.write().unwrap();
        let pool = pools.entry(mode).or_default();

        if !pool.contains(&uuid) {
            pool.push_back(uuid);
        }

        if pool.len() >= needed {
            let drained: Vec<String> = pool.drain(..needed).collect();
            EnqueueResult::MatchReady(drained)
        } else {
            EnqueueResult::Waiting {
                in_queue: pool.len(),
                needed,
            }
        }
    }

    pub fn dequeue(&self, mode: QueueMode, uuid: &str) -> bool {
        let mut pools = self.pools.write().unwrap();
        if let Some(pool) = pools.get_mut(&mode) {
            let before = pool.len();
            pool.retain(|id| id != uuid);
            return pool.len() != before;
        }
        false
    }

    /// Remove this uuid from every pool, regardless of mode. Useful for
    /// cleanup when we can't rely on a leave event.
    pub fn dequeue_all(&self, uuid: &str) {
        let mut pools = self.pools.write().unwrap();
        for pool in pools.values_mut() {
            pool.retain(|id| id != uuid);
        }
    }

    pub fn queue_position(&self, mode: QueueMode, uuid: &str) -> Option<usize> {
        let pools = self.pools.read().unwrap();
        pools.get(&mode)?.iter().position(|id| id == uuid)
    }
}

impl Default for QueueManager {
    fn default() -> Self {
        Self::new()
    }
}
