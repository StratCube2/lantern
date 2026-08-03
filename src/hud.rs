//! Lobby-side action bar HUD.
//!
//! `matches.rs::update_hud` already covers the in-match action bar (shown
//! while `PlayerState::InMatch`). This is the other half: for everyone
//! *not* currently fighting, show either which mode they're queued for
//! (`PlayerState::Queued`) or their idle stats line (wins/losses/kills/
//! tier) while sitting in the lobby (`PlayerState::Lobby`). Spectators
//! (`PlayerState::Spectating`) are left alone here since they're still
//! inside a running match and get its HUD instead.
//!
//! Scheduled once from `lib.rs::on_load` as a repeating task, same 20-tick
//! (1s) cadence the architecture doc uses for lobby-side UI refreshes (the
//! in-match HUD polls faster, at 10 ticks, since it doubles as the combat
//! loop).

use std::sync::Arc;

use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{Server, uuid};

use crate::commands::queue_cmd::mode_label;
use crate::state::{PlayerState, StateRegistry};
use crate::stats::StatsRegistry;
use crate::tiers::TierRegistry;

/// Poll interval for the lobby HUD, in ticks. 20 ticks == 1s.
pub const LOBBY_HUD_PERIOD_TICKS: u64 = 20;

pub fn tick_lobby_hud(server: &Server, state: &StateRegistry, stats: &StatsRegistry, tiers: &TierRegistry) {
    for player in server.get_all_players() {
        let uuid_str = uuid::to_string(player.get_id());
        let Some(current) = state.get(&uuid_str) else {
            // No entry yet (e.g. hasn't been touched by PlayerJoinEvent's
            // lobby-teleport logic) -- nothing to show.
            continue;
        };

        match current {
            PlayerState::Queued { mode } => {
                player.show_actionbar(TextComponent::text(&format!(
                    "Searching for {}...",
                    mode_label(&mode)
                )));
            }
            PlayerState::Lobby => {
                let s = stats.get(&uuid_str);
                let tier_name = tiers
                    .tier_for_wins(s.wins)
                    .map(|t| t.name)
                    .unwrap_or_else(|| "Unranked".to_string());
                player.show_actionbar(TextComponent::text(&format!(
                    "Wins: {} | Losses: {} | Kills: {} | Tier: {tier_name}",
                    s.wins, s.losses, s.kills
                )));
            }
            // In-match/spectating fighters get their action bar from
            // matches.rs::update_hud instead -- don't fight over the bar
            // with two different renderers every tick.
            PlayerState::InMatch { .. } | PlayerState::Spectating { .. } => {}
        }
    }
}
