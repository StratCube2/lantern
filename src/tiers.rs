//! Level/Tier system.
//!
//! A `Tier` is a named phase (e.g. "Bronze", "Silver", "Gold") gated by a
//! win requirement. Tiers are ordered by their `req` (wins needed) into
//! ascending phases: a player is considered to be in the *highest* tier
//! whose requirement their total win count meets or exceeds, and — per the
//! brief — "loses" every lower tier once they qualify for a higher one, i.e.
//! a player is only ever in exactly one tier at a time (the best one they
//! qualify for), not a stack of all tiers they've passed.
//!
//! NOTE: this was originally kill-gated, but there is no combat/damage event
//! exposed to plugins (see `matches.rs`'s module doc), so kills are never
//! actually recorded. Tiers are therefore gated on **wins** instead, which
//! *are* reliably detected (via the health-poll elimination loop). The field
//! is still named `req` generically so the persisted `tiers.toml` format and
//! the command surface don't need to change, but every caller now feeds it
//! win counts, not kill counts.
//!
//! Persisted to `tiers.toml` in the plugin's data folder.

use std::collections::BTreeMap;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tier {
    pub name: String,
    /// Wins required to reach this tier. (Named generically as `req`, but
    /// see the module doc for why this is wins and not kills.)
    pub req: u32,
}

pub struct TierRegistry {
    inner: RwLock<BTreeMap<String, Tier>>,
}

impl TierRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn list(&self) -> Vec<Tier> {
        self.inner.read().unwrap().values().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<Tier> {
        self.inner.read().unwrap().get(name).cloned()
    }

    /// Creates a new tier with a default requirement of 0 kills, if one
    /// doesn't already exist by that name. Use `set_requirement` afterward
    /// to configure it.
    pub fn create(&self, name: &str) -> bool {
        let mut map = self.inner.write().unwrap();
        if map.contains_key(name) {
            return false;
        }
        map.insert(
            name.to_string(),
            Tier {
                name: name.to_string(),
                req: 0,
            },
        );
        true
    }

    pub fn set_requirement(&self, name: &str, req: u32) -> bool {
        let mut map = self.inner.write().unwrap();
        let Some(tier) = map.get_mut(name) else {
            return false;
        };
        tier.req = req;
        true
    }

    pub fn delete(&self, name: &str) -> bool {
        self.inner.write().unwrap().remove(name).is_some()
    }

    /// Returns the highest tier whose requirement `wins` meets or exceeds,
    /// i.e. the single "current phase" a player is in. `None` if no tier's
    /// requirement is met yet (or no tiers exist).
    pub fn tier_for_wins(&self, wins: u32) -> Option<Tier> {
        self.inner
            .read()
            .unwrap()
            .values()
            .filter(|t| wins >= t.req)
            .max_by_key(|t| t.req)
            .cloned()
    }

    /// Returns the next tier above the player's current wins, if any, along
    /// with how many more wins are needed to reach it.
    pub fn next_tier_for_wins(&self, wins: u32) -> Option<(Tier, u32)> {
        self.inner
            .read()
            .unwrap()
            .values()
            .filter(|t| t.req > wins)
            .min_by_key(|t| t.req)
            .map(|t| (t.clone(), t.req - wins))
    }

    fn file_path(data_folder: &str) -> String {
        format!("{data_folder}/tiers.toml")
    }

    pub fn load(&self, data_folder: &str) {
        let path = Self::file_path(data_folder);
        let Ok(contents) = std::fs::read_to_string(&path) else {
            return;
        };
        #[derive(Deserialize)]
        struct TiersFile {
            #[serde(default)]
            tiers: Vec<Tier>,
        }
        match toml::from_str::<TiersFile>(&contents) {
            Ok(file) => {
                let mut map = self.inner.write().unwrap();
                for tier in file.tiers {
                    map.insert(tier.name.clone(), tier);
                }
            }
            Err(e) => tracing::warn!("Failed to parse tiers.toml: {e}"),
        }
    }

    pub fn save(&self, data_folder: &str) {
        #[derive(Serialize)]
        struct TiersFile {
            tiers: Vec<Tier>,
        }
        let file = TiersFile {
            tiers: self.list(),
        };
        let path = Self::file_path(data_folder);
        match toml::to_string_pretty(&file) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&path, text) {
                    tracing::warn!("Failed to write tiers.toml: {e}");
                }
            }
            Err(e) => tracing::warn!("Failed to serialize tiers.toml: {e}"),
        }
    }
}

impl Default for TierRegistry {
    fn default() -> Self {
        Self::new()
    }
}
