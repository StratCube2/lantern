//! Arena management.
//!
//! An `Arena` is a physical, admin-defined location pair (Team A / Team B
//! spawn points) used to host one match at a time. Boundaries are not
//! tracked here — per the design brief, arenas are expected to be walled off
//! in the world itself, so we don't do any out-of-bounds detection.
//!
//! Arenas carry their own small state machine (`ArenaState`) mirroring the
//! match lifecycle described in the architecture doc (Idle -> Countdown ->
//! Fighting -> Ending), which both prevents double-booking a single arena
//! and gives `/arena list`/admin tooling something concrete to report.
//!
//! Persisted to `arenas.toml` in the plugin's data folder.

use std::collections::BTreeMap;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Location {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

/// Mirrors the match lifecycle from the architecture doc, scoped to a single
/// arena so we know at a glance whether it's free to book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArenaState {
    Idle,
    Countdown,
    Fighting,
    Ending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arena {
    pub name: String,
    pub world_id: Option<String>,
    pub spawn_a: Option<Location>,
    pub spawn_b: Option<Location>,
    #[serde(default = "default_arena_state")]
    pub state: ArenaState,
}

fn default_arena_state() -> ArenaState {
    ArenaState::Idle
}

impl Arena {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            world_id: None,
            spawn_a: None,
            spawn_b: None,
            state: ArenaState::Idle,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.spawn_a.is_some() && self.spawn_b.is_some() && self.world_id.is_some()
    }

    pub fn is_free(&self) -> bool {
        self.state == ArenaState::Idle
    }
}

pub struct ArenaRegistry {
    inner: RwLock<BTreeMap<String, Arena>>,
}

impl ArenaRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn list(&self) -> Vec<Arena> {
        self.inner.read().unwrap().values().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<Arena> {
        self.inner.read().unwrap().get(name).cloned()
    }

    pub fn create(&self, name: &str) -> bool {
        let mut map = self.inner.write().unwrap();
        if map.contains_key(name) {
            return false;
        }
        map.insert(name.to_string(), Arena::new(name));
        true
    }

    pub fn delete(&self, name: &str) -> bool {
        self.inner.write().unwrap().remove(name).is_some()
    }

    pub fn rename(&self, old_name: &str, new_name: &str) -> bool {
        let mut map = self.inner.write().unwrap();
        if map.contains_key(new_name) {
            return false;
        }
        let Some(mut arena) = map.remove(old_name) else {
            return false;
        };
        arena.name = new_name.to_string();
        map.insert(new_name.to_string(), arena);
        true
    }

    pub fn set_spawn_a(&self, name: &str, world_id: String, loc: Location) -> bool {
        let mut map = self.inner.write().unwrap();
        let Some(arena) = map.get_mut(name) else {
            return false;
        };
        arena.world_id = Some(world_id);
        arena.spawn_a = Some(loc);
        true
    }

    pub fn set_spawn_b(&self, name: &str, world_id: String, loc: Location) -> bool {
        let mut map = self.inner.write().unwrap();
        let Some(arena) = map.get_mut(name) else {
            return false;
        };
        arena.world_id = Some(world_id);
        arena.spawn_b = Some(loc);
        true
    }

    pub fn set_state(&self, name: &str, state: ArenaState) {
        let mut map = self.inner.write().unwrap();
        if let Some(arena) = map.get_mut(name) {
            arena.state = state;
        }
    }

    /// Finds and reserves (marks `Countdown`) the first ready + free arena.
    /// Returns its name if one was found and reserved.
    pub fn reserve_free_arena(&self) -> Option<String> {
        let mut map = self.inner.write().unwrap();
        let found = map
            .values()
            .find(|a| a.is_ready() && a.is_free())
            .map(|a| a.name.clone())?;
        if let Some(arena) = map.get_mut(&found) {
            arena.state = ArenaState::Countdown;
        }
        Some(found)
    }

    /// Releases an arena back to `Idle`, freeing it for the next match.
    pub fn release(&self, name: &str) {
        self.set_state(name, ArenaState::Idle);
    }

    fn file_path(data_folder: &str) -> String {
        format!("{data_folder}/arenas.toml")
    }

    pub fn load(&self, data_folder: &str) {
        let path = Self::file_path(data_folder);
        let Ok(contents) = std::fs::read_to_string(&path) else {
            return;
        };
        #[derive(Deserialize)]
        struct ArenasFile {
            #[serde(default)]
            arenas: Vec<Arena>,
        }
        match toml::from_str::<ArenasFile>(&contents) {
            Ok(file) => {
                let mut map = self.inner.write().unwrap();
                for mut arena in file.arenas {
                    // Never resume as occupied across a restart -- any match
                    // that was running is gone now.
                    arena.state = ArenaState::Idle;
                    map.insert(arena.name.clone(), arena);
                }
            }
            Err(e) => tracing::warn!("Failed to parse arenas.toml: {e}"),
        }
    }

    pub fn save(&self, data_folder: &str) {
        #[derive(Serialize)]
        struct ArenasFile {
            arenas: Vec<Arena>,
        }
        let file = ArenasFile {
            arenas: self.list(),
        };
        let path = Self::file_path(data_folder);
        match toml::to_string_pretty(&file) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&path, text) {
                    tracing::warn!("Failed to write arenas.toml: {e}");
                }
            }
            Err(e) => tracing::warn!("Failed to serialize arenas.toml: {e}"),
        }
    }
}

impl Default for ArenaRegistry {
    fn default() -> Self {
        Self::new()
    }
}
