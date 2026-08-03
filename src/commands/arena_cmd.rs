//! `/arena create <name>`, `/arena delete <name>`, `/arena rename <old> <new>`,
//! `/arena setposa <name>`, `/arena setposb <name>`, `/arena list`.
//!
//! Boundaries are intentionally not tracked (per the design brief: arenas
//! are expected to be walled off in the world itself), so this only ever
//! saves two spawn points + a world id, matching `arena.rs`'s `Arena`
//! struct.

use std::sync::Arc;

use pumpkin_plugin_api::{
    Server,
    command::{Arg, CommandError, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    text::TextComponent,
};

use crate::arena::{ArenaRegistry, Location};

fn get_string_arg(args: &ConsumedArgs, name: &str) -> Option<String> {
    match args.get_value(name) {
        Arg::Simple(s) => Some(s),
        _ => None,
    }
}

pub struct ArenaCreateExecutor {
    pub arenas: Arc<ArenaRegistry>,
    pub data_folder: String,
}

impl CommandHandler for ArenaCreateExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(name) = get_string_arg(&args, "name") else {
            sender.send_message(TextComponent::text("Usage: /arena create <name>"));
            return Ok(0);
        };

        if !self.arenas.create(&name) {
            sender.send_message(TextComponent::text(&format!("An arena named '{name}' already exists.")));
            return Ok(0);
        }
        self.arenas.save(&self.data_folder);
        sender.send_message(TextComponent::text(&format!(
            "Arena '{name}' created. Stand at Team A's spawn and run /arena setposa {name}, \
             then at Team B's spawn run /arena setposb {name}."
        )));
        Ok(1)
    }
}

pub struct ArenaDeleteExecutor {
    pub arenas: Arc<ArenaRegistry>,
    pub data_folder: String,
}

impl CommandHandler for ArenaDeleteExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(name) = get_string_arg(&args, "name") else {
            sender.send_message(TextComponent::text("Usage: /arena delete <name>"));
            return Ok(0);
        };

        if !self.arenas.delete(&name) {
            sender.send_message(TextComponent::text(&format!("No arena named '{name}'.")));
            return Ok(0);
        }
        self.arenas.save(&self.data_folder);
        sender.send_message(TextComponent::text(&format!("Arena '{name}' deleted.")));
        Ok(1)
    }
}

pub struct ArenaRenameExecutor {
    pub arenas: Arc<ArenaRegistry>,
    pub data_folder: String,
}

impl CommandHandler for ArenaRenameExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let (Some(old_name), Some(new_name)) =
            (get_string_arg(&args, "name"), get_string_arg(&args, "new_name"))
        else {
            sender.send_message(TextComponent::text("Usage: /arena rename <name> <new_name>"));
            return Ok(0);
        };

        if !self.arenas.rename(&old_name, &new_name) {
            sender.send_message(TextComponent::text(&format!(
                "Couldn't rename '{old_name}' to '{new_name}' — either it doesn't exist or that name is taken."
            )));
            return Ok(0);
        }
        self.arenas.save(&self.data_folder);
        sender.send_message(TextComponent::text(&format!("Arena '{old_name}' renamed to '{new_name}'.")));
        Ok(1)
    }
}

pub struct ArenaSetPosAExecutor {
    pub arenas: Arc<ArenaRegistry>,
    pub data_folder: String,
}

impl CommandHandler for ArenaSetPosAExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            sender.send_message(TextComponent::text("Only players can set an arena spawn point."));
            return Ok(0);
        };
        let Some(name) = get_string_arg(&args, "name") else {
            sender.send_message(TextComponent::text("Usage: /arena setposa <name>"));
            return Ok(0);
        };

        let (x, y, z) = player.get_position();
        let loc = Location {
            x,
            y,
            z,
            yaw: player.get_yaw(),
            pitch: player.get_pitch(),
        };
        let world_id = player.get_world().get_id();

        if !self.arenas.set_spawn_a(&name, world_id, loc) {
            sender.send_message(TextComponent::text(&format!("No arena named '{name}'.")));
            return Ok(0);
        }
        self.arenas.save(&self.data_folder);
        sender.send_message(TextComponent::text(&format!("Team A spawn for '{name}' set.")));
        Ok(1)
    }
}

pub struct ArenaSetPosBExecutor {
    pub arenas: Arc<ArenaRegistry>,
    pub data_folder: String,
}

impl CommandHandler for ArenaSetPosBExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            sender.send_message(TextComponent::text("Only players can set an arena spawn point."));
            return Ok(0);
        };
        let Some(name) = get_string_arg(&args, "name") else {
            sender.send_message(TextComponent::text("Usage: /arena setposb <name>"));
            return Ok(0);
        };

        let (x, y, z) = player.get_position();
        let loc = Location {
            x,
            y,
            z,
            yaw: player.get_yaw(),
            pitch: player.get_pitch(),
        };
        let world_id = player.get_world().get_id();

        if !self.arenas.set_spawn_b(&name, world_id, loc) {
            sender.send_message(TextComponent::text(&format!("No arena named '{name}'.")));
            return Ok(0);
        }
        self.arenas.save(&self.data_folder);
        sender.send_message(TextComponent::text(&format!("Team B spawn for '{name}' set.")));
        Ok(1)
    }
}

pub struct ArenaListExecutor {
    pub arenas: Arc<ArenaRegistry>,
}

impl CommandHandler for ArenaListExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        let arenas = self.arenas.list();
        if arenas.is_empty() {
            sender.send_message(TextComponent::text("No arenas have been created yet."));
            return Ok(0);
        }

        let mut lines = vec![format!("Arenas ({}):", arenas.len())];
        for arena in &arenas {
            let status = if !arena.is_ready() {
                "not fully set up"
            } else if arena.is_free() {
                "free"
            } else {
                "in use"
            };
            lines.push(format!("- {} ({status})", arena.name));
        }
        sender.send_message(TextComponent::text(&lines.join("\n")));
        Ok(1)
    }
}
