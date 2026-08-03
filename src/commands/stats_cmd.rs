//! `/stats [player]` — shows wins/losses/winstreak/kills/deaths/K-D/W-L for
//! yourself or another player.
//! `/leaderboard [category]` — top 10 players sorted by a stat category.

use std::sync::Arc;

use pumpkin_plugin_api::{
    Server,
    command::{Arg, CommandError, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    text::TextComponent,
    uuid,
};

use crate::stats::{LeaderboardSort, StatsRegistry};

const LEADERBOARD_SIZE: usize = 10;

pub struct StatsSelfExecutor {
    pub stats: Arc<StatsRegistry>,
}

impl CommandHandler for StatsSelfExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            sender.send_message(TextComponent::text("Usage (console): /stats <player>"));
            return Ok(0);
        };
        let uuid_str = uuid::to_string(player.get_id());
        let s = self.stats.get(&uuid_str);
        sender.send_message(TextComponent::text(&format_stats(&player.get_name(), &s)));
        Ok(1)
    }
}

pub struct StatsOtherExecutor {
    pub stats: Arc<StatsRegistry>,
}

impl CommandHandler for StatsOtherExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let name = match args.get_value("player") {
            Arg::Simple(s) => s,
            _ => {
                sender.send_message(TextComponent::text("Usage: /stats <player>"));
                return Ok(0);
            }
        };

        let Some((_uuid, s)) = self.stats.get_by_name(&name) else {
            sender.send_message(TextComponent::text(&format!("No stats recorded for '{name}' yet.")));
            return Ok(0);
        };
        sender.send_message(TextComponent::text(&format_stats(&name, &s)));
        Ok(1)
    }
}

fn format_stats(name: &str, s: &crate::stats::PlayerStats) -> String {
    format!(
        "{name}'s stats:\n\
         Wins: {} | Losses: {} | Winstreak: {} (best {})\n\
         Kills: {} | Deaths: {}\n\
         K/D: {:.2} | W/L: {:.2}",
        s.wins, s.losses, s.winstreak, s.best_winstreak, s.kills, s.deaths, s.kd(), s.wl()
    )
}

pub struct LeaderboardExecutor {
    pub stats: Arc<StatsRegistry>,
    /// Default category when `/leaderboard` is run with no argument.
    pub default_sort: LeaderboardSort,
}

impl CommandHandler for LeaderboardExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        render_leaderboard(&sender, &self.stats, self.default_sort);
        Ok(1)
    }
}

pub struct LeaderboardCategoryExecutor {
    pub stats: Arc<StatsRegistry>,
}

impl CommandHandler for LeaderboardCategoryExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let category = match args.get_value("category") {
            Arg::Simple(s) => s,
            _ => {
                sender.send_message(TextComponent::text(
                    "Usage: /leaderboard <wins|losses|winstreak|kills|deaths|kd|wl>",
                ));
                return Ok(0);
            }
        };
        let Some(sort) = LeaderboardSort::parse(&category) else {
            sender.send_message(TextComponent::text(&format!(
                "Unknown category '{category}'. Try: wins, losses, winstreak, kills, deaths, kd, wl"
            )));
            return Ok(0);
        };
        render_leaderboard(&sender, &self.stats, sort);
        Ok(1)
    }
}

fn render_leaderboard(sender: &CommandSender, stats: &StatsRegistry, sort: LeaderboardSort) {
    let top = stats.leaderboard(sort, LEADERBOARD_SIZE);
    if top.is_empty() {
        sender.send_message(TextComponent::text("No stats recorded yet."));
        return;
    }

    let mut lines = vec![format!("Top {} — {}:", top.len(), sort.label())];
    for (i, (_uuid, s)) in top.iter().enumerate() {
        let value = match sort {
            LeaderboardSort::Wins => s.wins as f64,
            LeaderboardSort::Losses => s.losses as f64,
            LeaderboardSort::Winstreak => s.winstreak as f64,
            LeaderboardSort::Kills => s.kills as f64,
            LeaderboardSort::Deaths => s.deaths as f64,
            LeaderboardSort::Kd => s.kd(),
            LeaderboardSort::Wl => s.wl(),
        };
        lines.push(format!("{}. {} — {:.2}", i + 1, s.name, value));
    }
    sender.send_message(TextComponent::text(&lines.join("\n")));
}
