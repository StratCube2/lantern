//! Player statistics: wins, losses, winstreak, kills, deaths, and the
//! derived K/D and W/L ratios used by `/stats` and `/leaderboard`.
//!
//! Persisted to `stats.toml` in the plugin's data folder. Kept as a flat
//! uuid -> record map, same storage pattern as `state.rs`/`kits.rs`.

use std::collections::HashMap;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerStats {
    /// Last known username, kept alongside the uuid key so leaderboards and
    /// `/stats <player>` don't require the target to be online.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub wins: u32,
    #[serde(default)]
    pub losses: u32,
    #[serde(default)]
    pub winstreak: u32,
    #[serde(default)]
    pub best_winstreak: u32,
    #[serde(default)]
    pub kills: u32,
    #[serde(default)]
    pub deaths: u32,
}

impl PlayerStats {
    pub fn kd(&self) -> f64 {
        if self.deaths == 0 {
            self.kills as f64
        } else {
            self.kills as f64 / self.deaths as f64
        }
    }

    pub fn wl(&self) -> f64 {
        if self.losses == 0 {
            self.wins as f64
        } else {
            self.wins as f64 / self.losses as f64
        }
    }
}

pub struct StatsRegistry {
    inner: RwLock<HashMap<String, PlayerStats>>,
}

impl StatsRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    pub fn get(&self, uuid: &str) -> PlayerStats {
        self.inner.read().unwrap().get(uuid).cloned().unwrap_or_default()
    }

    pub fn get_by_name(&self, name: &str) -> Option<(String, PlayerStats)> {
        self.inner
            .read()
            .unwrap()
            .iter()
            .find(|(_, s)| s.name.eq_ignore_ascii_case(name))
            .map(|(uuid, s)| (uuid.clone(), s.clone()))
    }

    pub fn touch_name(&self, uuid: &str, name: &str) {
        let mut map = self.inner.write().unwrap();
        let entry = map.entry(uuid.to_string()).or_default();
        entry.name = name.to_string();
    }

    /// Records a win for `uuid`, bumping winstreak and its best-ever value.
    pub fn record_win(&self, uuid: &str, name: &str) {
        let mut map = self.inner.write().unwrap();
        let entry = map.entry(uuid.to_string()).or_default();
        entry.name = name.to_string();
        entry.wins += 1;
        entry.winstreak += 1;
        if entry.winstreak > entry.best_winstreak {
            entry.best_winstreak = entry.winstreak;
        }
    }

    /// Records a loss for `uuid`, resetting winstreak to zero.
    pub fn record_loss(&self, uuid: &str, name: &str) {
        let mut map = self.inner.write().unwrap();
        let entry = map.entry(uuid.to_string()).or_default();
        entry.name = name.to_string();
        entry.losses += 1;
        entry.winstreak = 0;
    }

    pub fn record_kill(&self, uuid: &str, name: &str) {
        let mut map = self.inner.write().unwrap();
        let entry = map.entry(uuid.to_string()).or_default();
        entry.name = name.to_string();
        entry.kills += 1;
    }

    pub fn record_death(&self, uuid: &str, name: &str) {
        let mut map = self.inner.write().unwrap();
        let entry = map.entry(uuid.to_string()).or_default();
        entry.name = name.to_string();
        entry.deaths += 1;
    }

    /// Wipes a single player's stats. Used by `/lantern stats reset <player>`.
    pub fn reset(&self, uuid: &str) -> bool {
        self.inner.write().unwrap().remove(uuid).is_some()
    }

    /// Returns the top `limit` players sorted by the given key.
    pub fn leaderboard(&self, sort_by: LeaderboardSort, limit: usize) -> Vec<(String, PlayerStats)> {
        let map = self.inner.read().unwrap();
        let mut all: Vec<(String, PlayerStats)> =
            map.iter().map(|(uuid, s)| (uuid.clone(), s.clone())).collect();
        all.sort_by(|a, b| sort_by.key(&b.1).partial_cmp(&sort_by.key(&a.1)).unwrap());
        all.truncate(limit);
        all
    }

    fn file_path(data_folder: &str) -> String {
        format!("{data_folder}/stats.toml")
    }

    pub fn load(&self, data_folder: &str) {
        let path = Self::file_path(data_folder);
        let Ok(contents) = std::fs::read_to_string(&path) else {
            return;
        };
        #[derive(Deserialize)]
        struct StatsFile {
            #[serde(default)]
            players: HashMap<String, PlayerStats>,
        }
        match toml::from_str::<StatsFile>(&contents) {
            Ok(file) => {
                *self.inner.write().unwrap() = file.players;
            }
            Err(e) => tracing::warn!("Failed to parse stats.toml: {e}"),
        }
    }

    pub fn save(&self, data_folder: &str) {
        #[derive(Serialize)]
        struct StatsFile {
            players: HashMap<String, PlayerStats>,
        }
        let file = StatsFile {
            players: self.inner.read().unwrap().clone(),
        };
        let path = Self::file_path(data_folder);
        match toml::to_string_pretty(&file) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&path, text) {
                    tracing::warn!("Failed to write stats.toml: {e}");
                }
            }
            Err(e) => tracing::warn!("Failed to serialize stats.toml: {e}"),
        }
    }
}

impl Default for StatsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderboardSort {
    Wins,
    Losses,
    Winstreak,
    Kills,
    Deaths,
    Kd,
    Wl,
}

impl LeaderboardSort {
    fn key(&self, s: &PlayerStats) -> f64 {
        match self {
            LeaderboardSort::Wins => s.wins as f64,
            LeaderboardSort::Losses => s.losses as f64,
            LeaderboardSort::Winstreak => s.winstreak as f64,
            LeaderboardSort::Kills => s.kills as f64,
            LeaderboardSort::Deaths => s.deaths as f64,
            LeaderboardSort::Kd => s.kd(),
            LeaderboardSort::Wl => s.wl(),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "wins" | "win" => Some(Self::Wins),
            "losses" | "loss" | "loses" => Some(Self::Losses),
            "winstreak" | "streak" => Some(Self::Winstreak),
            "kills" | "kill" => Some(Self::Kills),
            "deaths" | "death" => Some(Self::Deaths),
            "kd" | "k/d" => Some(Self::Kd),
            "wl" | "w/l" => Some(Self::Wl),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            LeaderboardSort::Wins => "Wins",
            LeaderboardSort::Losses => "Losses",
            LeaderboardSort::Winstreak => "Winstreak",
            LeaderboardSort::Kills => "Kills",
            LeaderboardSort::Deaths => "Deaths",
            LeaderboardSort::Kd => "K/D",
            LeaderboardSort::Wl => "W/L",
        }
    }
}
