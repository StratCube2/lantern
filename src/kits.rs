//! Kit registry.
//!
//! A `Kit` is a named practice loadout: an icon item (shown in the `/gm`
//! picker), plus the hotbar/armor contents given to players when a match
//! using that kit starts. `/lantern` creates/edits kits; `/gm` lists them
//! and queues the clicker for whichever one they click.
//!
//! Persisted to `kits.toml` in the plugin's data folder so kits survive a
//! server restart.

use std::collections::BTreeMap;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

/// A single item placement within a kit — kept as (registry key, count)
/// rather than a full `ItemStack`, since `ItemStack` is a live WIT resource
/// (not serializable) and we only need enough to reconstruct one with
/// `ItemStack::new(key, count)` when the kit is actually equipped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KitItem {
    pub registry_key: String,
    pub count: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kit {
    pub name: String,
    /// Registry key for the item shown as this kit's icon in the /gm menu
    /// (e.g. "minecraft:diamond_sword").
    pub icon: String,
    /// Hotbar/inventory slot -> item, keyed by slot index (0-35 typical
    /// player inventory range; kept slot-agnostic here since /lantern is
    /// what decides what counts as a valid slot).
    pub items: BTreeMap<u8, KitItem>,
    /// If true, this kit queues in groups larger than 2 (XvX / party-style)
    /// rather than a plain 1v1. Toggled with
    /// `/lantern kit "<name>" multiplayer true|false`.
    #[serde(default)]
    pub multiplayer: bool,
    /// Number of players per team when `multiplayer` is enabled. Ignored
    /// for 1v1 kits (team size is always 1 there).
    #[serde(default = "default_team_size")]
    pub team_size: u8,
    /// Whether fall damage is enabled for matches using this kit. Since the
    /// host doesn't expose a gamerule setter to plugins, this is enforced by
    /// the match loop (fall damage is measured and reverted on impact when
    /// disabled) rather than a literal gamerule toggle.
    #[serde(default = "default_true")]
    pub fall_damage: bool,
    /// Whether the 3-second pre-fight countdown runs for this kit. When
    /// false, players are dropped straight into the fight after teleporting.
    #[serde(default = "default_true")]
    pub countdown: bool,
}

fn default_team_size() -> u8 {
    2
}

fn default_true() -> bool {
    true
}

impl Kit {
    pub fn new(name: impl Into<String>, icon: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            icon: icon.into(),
            items: BTreeMap::new(),
            multiplayer: false,
            team_size: default_team_size(),
            fall_damage: true,
            countdown: true,
        }
    }

    /// Total players required to start a match with this kit.
    pub fn required_players(&self) -> usize {
        if self.multiplayer {
            (self.team_size as usize).max(1) * 2
        } else {
            2
        }
    }
}

pub struct KitRegistry {
    inner: RwLock<BTreeMap<String, Kit>>,
}

impl KitRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn list(&self) -> Vec<Kit> {
        self.inner.read().unwrap().values().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<Kit> {
        self.inner.read().unwrap().get(name).cloned()
    }

    pub fn upsert(&self, kit: Kit) {
        self.inner.write().unwrap().insert(kit.name.clone(), kit);
    }

    pub fn remove(&self, name: &str) -> bool {
        self.inner.write().unwrap().remove(name).is_some()
    }

    pub fn rename(&self, old_name: &str, new_name: &str) -> bool {
        let mut map = self.inner.write().unwrap();
        if map.contains_key(new_name) {
            return false;
        }
        let Some(mut kit) = map.remove(old_name) else {
            return false;
        };
        kit.name = new_name.to_string();
        map.insert(new_name.to_string(), kit);
        true
    }

    fn file_path(data_folder: &str) -> String {
        format!("{data_folder}/kits.toml")
    }

    pub fn load(&self, data_folder: &str) {
        let path = Self::file_path(data_folder);
        let Ok(contents) = std::fs::read_to_string(&path) else {
            return;
        };
        #[derive(Deserialize)]
        struct KitsFile {
            #[serde(default)]
            kits: Vec<Kit>,
        }
        match toml::from_str::<KitsFile>(&contents) {
            Ok(file) => {
                let mut map = self.inner.write().unwrap();
                for kit in file.kits {
                    map.insert(kit.name.clone(), kit);
                }
            }
            Err(e) => tracing::warn!("Failed to parse kits.toml: {e}"),
        }
    }

    pub fn save(&self, data_folder: &str) {
        #[derive(Serialize)]
        struct KitsFile {
            kits: Vec<Kit>,
        }
        let file = KitsFile {
            kits: self.list(),
        };
        let path = Self::file_path(data_folder);
        match toml::to_string_pretty(&file) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&path, text) {
                    tracing::warn!("Failed to write kits.toml: {e}");
                }
            }
            Err(e) => tracing::warn!("Failed to serialize kits.toml: {e}"),
        }
    }
}

impl Default for KitRegistry {
    fn default() -> Self {
        Self::new()
    }
}
