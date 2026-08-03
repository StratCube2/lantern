//! `/queue <kit>` — queue for a named, admin-created kit.
//! `/dequeue` — leave whatever queue you're in.
//!
//! `/gm` still opens a GUI, but the command path queues directly by kit
//! identifier so players do not have to browse the menu first.
//!
//! This is the wiring point where a full queue pool actually turns into a
//! running fight: once `QueueManager::enqueue` returns `MatchReady`, we hand
//! the roster + kit straight to `MatchManager::try_start_match`. If no arena
//! is free, per the architecture doc's "Searching for open arena..."
//! behavior, the players are put right back in queue and told via action
//! bar rather than being dropped.

use std::sync::Arc;

use pumpkin_plugin_api::{
    Server,
    command::{CommandError, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    text::TextComponent,
    uuid,
};

use crate::kits::{Kit, KitRegistry};
use crate::matches::MatchManager;
use crate::queue::{EnqueueResult, QueueManager};
use crate::state::{PlayerState, QueueMode, StateRegistry};

pub struct QueueHelpExecutor {
    pub message: String,
}

impl CommandHandler for QueueHelpExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        sender.send_message(TextComponent::text(&self.message));
        Ok(0)
    }
}

pub struct QueueKitExecutor {
    pub kits: Arc<KitRegistry>,
    pub state: Arc<StateRegistry>,
    pub queue: Arc<QueueManager>,
    pub matches: Arc<MatchManager>,
}

impl CommandHandler for QueueKitExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            sender.send_message(TextComponent::text("Only players can join a queue."));
            return Ok(0);
        };

        let kit_name = match args.get_value("kit") {
            pumpkin_plugin_api::command::Arg::Simple(s) => s,
            _ => {
                sender.send_message(TextComponent::text("Usage: /queue <kit>"));
                return Ok(0);
            }
        };

        let Some(kit) = self.kits.get(&kit_name) else {
            sender.send_message(TextComponent::text(&format!(
                "Unknown kit '{kit_name}'. Use /gm to browse the available kits."
            )));
            return Ok(0);
        };

        queue_player_for_kit(&player, &kit, &self.state, &self.queue, &self.matches, &server)
    }
}

pub struct QueueLeaveExecutor {
    pub state: Arc<StateRegistry>,
    pub queue: Arc<QueueManager>,
}

impl CommandHandler for QueueLeaveExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(uuid) = sender_uuid(&sender) else {
            return Ok(0);
        };

        match self.state.get_or_init(&uuid) {
            PlayerState::Queued { .. } => {
                self.queue.dequeue_all(&uuid);
                self.state.set(&uuid, PlayerState::Lobby);
                sender.send_message(TextComponent::text("Left the queue."));
                Ok(1)
            }
            _ => {
                sender.send_message(TextComponent::text("You're not in a queue."));
                Ok(0)
            }
        }
    }
}

/// Entry point used by both `/queue <kit>` and the `/gm` GUI click handler.
pub fn queue_player_for_kit(
    player: &pumpkin_plugin_api::player::Player,
    kit: &Kit,
    state: &StateRegistry,
    queue: &QueueManager,
    matches: &Arc<MatchManager>,
    server: &Server,
) -> Result<i32, CommandError> {
    let uuid = uuid::to_string(player.get_id());
    let mode = QueueMode::Kit(kit.name.clone());

    match state.get_or_init(&uuid) {
        PlayerState::Lobby => {}
        PlayerState::Queued { .. } => {
            player.send_system_message(
                TextComponent::text("You're already in a queue. Use /dequeue first."),
                false,
            );
            return Ok(0);
        }
        PlayerState::InMatch { .. } => {
            player.send_system_message(TextComponent::text("You can't queue while in a match."), false);
            return Ok(0);
        }
        PlayerState::Spectating { .. } => {
            player.send_system_message(
                TextComponent::text("You can't queue while spectating."),
                false,
            );
            return Ok(0);
        }
    }

    state.set(&uuid, PlayerState::Queued { mode: mode.clone() });

    let needed = kit.required_players();
    match queue.enqueue(mode.clone(), uuid, needed) {
        EnqueueResult::Waiting { .. } => {
            // Action bar feedback ("Searching for <mode>...") is handled by
            // the periodic lobby HUD task (see lib.rs), which reads
            // `PlayerState::Queued` every tick -- a one-shot message here
            // would just get overwritten immediately.
        }
        EnqueueResult::MatchReady(players) => {
            if !matches.try_start_match(server, kit, &players) {
                // No arena was free (or someone vanished between queueing
                // and match start) -- requeue everyone rather than drop
                // them, and let them know via action bar per the
                // architecture doc's "Searching for open arena..." spec.
                for uuid_str in &players {
                    state.set(uuid_str, PlayerState::Queued { mode: mode.clone() });
                    let _ = queue.enqueue(mode.clone(), uuid_str.clone(), needed);
                    if let Some(parsed) = uuid::parse(uuid_str) {
                        if let Some(p) = server.get_player_by_uuid(parsed) {
                            p.show_actionbar(TextComponent::text("Searching for open arena..."));
                        }
                    }
                }
            }
        }
    }

    Ok(1)
}

pub fn mode_label(mode: &QueueMode) -> String {
    match mode {
        QueueMode::Kit(name) => name.clone(),
        QueueMode::NoDebuff1v1 => "nodebuff".to_string(),
        QueueMode::Sumo1v1 => "sumo".to_string(),
        QueueMode::Gapple1v1 => "gapple".to_string(),
        QueueMode::Teams2v2 => "teams".to_string(),
    }
}

// Resolves a stable string key for the caller, or None for console/RCON
// senders (who can't join a queue).
//
// Confirmed against pumpkin-plugin-wit v0.1:
//   command.wit -> command-sender.as-player() -> option<player>
//   player.wit  -> player.get-id() -> uuid
//   uuid.wit    -> uuid::to-string(id) -> string
fn sender_uuid(sender: &CommandSender) -> Option<String> {
    let player = sender.as_player()?;
    Some(uuid::to_string(player.get_id()))
}
