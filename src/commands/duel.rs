//! `/duel <player>` — direct player-vs-player challenge.
//!
//! `/duel` used to be the queue-join command; that's moved to `/queue` (see
//! commands/queue_cmd.rs) so `/duel` is free for its more literal meaning:
//! challenging one specific player rather than joining an anonymous pool.
//!
//! Not implemented yet — challenge/accept flow, a pending-challenge timeout,
//! and which kit a direct duel uses are all open questions. This stub
//! confirms the command registers and that the `players` argument type
//! (`argument-type::players` / `arg::players(list<player>)` in command.wit)
//! resolves to an actual online player, so the real flow can be built on top
//! without re-deriving the argument plumbing.

use pumpkin_plugin_api::{
    Server,
    command::{CommandError, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    text::TextComponent,
};

pub struct DuelChallengeExecutor;

impl CommandHandler for DuelChallengeExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if sender.as_player().is_none() {
            sender.send_message(TextComponent::text("Only players can challenge someone."));
            return Ok(0);
        }

        // NOTE: `get-value` returns `arg`, a variant; the `players` argument
        // type parses to `arg::players(list<player>)`. Matching on that here
        // to confirm target resolution, but not doing anything with it yet —
        // challenge/accept state isn't designed. If `arg` doesn't derive a
        // pattern-matchable Rust enum the way other wrapped resources do,
        // this is the first place to check when wiring the real flow.
        let target = args.get_value("target");
        match target {
            pumpkin_plugin_api::command::Arg::Players(players) if !players.is_empty() => {
                sender.send_message(TextComponent::text(&format!(
                    "Direct duel challenges aren't implemented yet — {} players matched, but there's nowhere to send the invite.",
                    players.len()
                )));
            }
            _ => {
                sender.send_message(TextComponent::text(
                    "Couldn't find that player, and direct duels aren't implemented yet regardless.",
                ));
            }
        }

        Ok(1)
    }
}
