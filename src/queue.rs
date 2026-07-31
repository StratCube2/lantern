//! Matchmaking queue pools. Phase 1 only needs enqueue/dequeue + threshold
//! checks; arena assignment and match start are Phase 4/5.

use crate::state::QueueMode;
use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;

pub struct QueueManager {
    pools: RwLock<HashMap<QueueMode, VecDeque<String>>>, // uuid queue per mode
}

impl QueueMode {
    pub fn required_players(&self) -> usize {
        match self {
            QueueMode::NoDebuff1v1 | QueueMode::Sumo1v1 | QueueMode::Gapple1v1 => 2,
            QueueMode::Teams2v2 => 4,
            // Kits created via /lantern are 1v1 by default. If team kits are
            // wanted later, this needs a per-kit player-count field on `Kit`
            // (kits.rs) rather than a hardcoded guess here.
            QueueMode::Kit(_) => 2,
        }
    }
}

pub enum EnqueueResult {
    /// Not enough players yet; still waiting.
    Waiting,
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

    pub fn enqueue(&self, mode: QueueMode, uuid: String) -> EnqueueResult {
        let needed = mode.required_players();
        let mut pools = self.pools.write().unwrap();
        let pool = pools.entry(mode).or_default();

        if !pool.contains(&uuid) {
            pool.push_back(uuid);
        }

        if pool.len() >= needed {
            let drained: Vec<String> = pool.drain(..needed).collect();
            EnqueueResult::MatchReady(drained)
        } else {
            EnqueueResult::Waiting
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
