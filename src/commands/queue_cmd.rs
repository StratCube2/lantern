//! `/queue <kit>` — queue for a named, admin-created kit.
//! `/dequeue` — leave whatever queue you're in.
//!
//! `/gm` still opens a GUI, but the command path now queues directly by kit
//! identifier so players do not have to browse the menu first.

use std::sync::Arc;

use pumpkin_plugin_api::{
    Server,
    command::{CommandError, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    text::TextComponent,
    uuid,
};

use crate::kits::KitRegistry;
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

        queue_player_for_kit(&player, &kit.name, &self.state, &self.queue, &server)
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

pub fn queue_player_for_kit(
    player: &pumpkin_plugin_api::player::Player,
    kit_name: &str,
    state: &StateRegistry,
    queue: &QueueManager,
    server: &Server,
) -> Result<i32, CommandError> {
    let uuid = uuid::to_string(player.get_id());
    queue_player_for_mode(player, server, &uuid, &QueueMode::Kit(kit_name.to_string()), state, queue)
}

fn queue_player_for_mode(
    player: &pumpkin_plugin_api::player::Player,
    server: &Server,
    uuid: &str,
    mode: &QueueMode,
    state: &StateRegistry,
    queue: &QueueManager,
) -> Result<i32, CommandError> {
    match state.get_or_init(uuid) {
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

    state.set(uuid, PlayerState::Queued { mode: mode.clone() });

    match queue.enqueue(mode.clone(), uuid.to_string()) {
        EnqueueResult::Waiting => {
            player.send_system_message(
                TextComponent::text(&format!("Queued for {}. Searching for an opponent...", mode_label(mode))),
                false,
            );
        }
        EnqueueResult::MatchReady(players) => {
            for uuid_str in &players {
                if let Some(parsed) = uuid::parse(uuid_str) {
                    if let Some(matched_player) = server.get_player_by_uuid(parsed) {
                        matched_player.send_system_message(
                            TextComponent::text(&format!(
                                "Queued for {}. Match found! Arena assignment coming in Phase 4.",
                                mode_label(mode)
                            )),
                            false,
                        );
                    }
                }
            }
        }
    }

    Ok(1)
}

fn mode_label(mode: &QueueMode) -> String {
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
