//! `/tier "<name>"` — create a new tier (phase).
//! `/tier "<name>" req <int>` — set how many wins that tier requires.
//!
//! Tiers are ordered purely by their `req` value; a player is considered to
//! be in the single highest tier whose requirement they meet (see
//! `tiers.rs` module doc for the "one phase at a time" semantics).

use std::sync::Arc;

use pumpkin_plugin_api::{
    Server,
    command::{Arg, CommandError, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    text::TextComponent,
};

use crate::tiers::TierRegistry;

fn get_string_arg(args: &ConsumedArgs, name: &str) -> Option<String> {
    match args.get_value(name) {
        Arg::Simple(s) => Some(s),
        _ => None,
    }
}

pub struct TierCreateExecutor {
    pub tiers: Arc<TierRegistry>,
    pub data_folder: String,
}

impl CommandHandler for TierCreateExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(name) = get_string_arg(&args, "name") else {
            sender.send_message(TextComponent::text("Usage: /tier \"<name>\""));
            return Ok(0);
        };

        if !self.tiers.create(&name) {
            sender.send_message(TextComponent::text(&format!("A tier named '{name}' already exists.")));
            return Ok(0);
        }
        self.tiers.save(&self.data_folder);
        sender.send_message(TextComponent::text(&format!(
            "Tier '{name}' created with a requirement of 0 wins. \
             Set it with /tier \"{name}\" req <wins>."
        )));
        Ok(1)
    }
}

pub struct TierReqExecutor {
    pub tiers: Arc<TierRegistry>,
    pub data_folder: String,
}

impl CommandHandler for TierReqExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(name) = get_string_arg(&args, "name") else {
            sender.send_message(TextComponent::text("Usage: /tier \"<name>\" req <wins>"));
            return Ok(0);
        };

        // `Number`/`NotInBounds` aren't re-exported from
        // `pumpkin_plugin_api::command` (only Arg/ArgumentType/... are, per
        // lib.rs), so we reach the raw WIT-generated type through
        // `command_wit` instead.
        let req = match args.get_value("req") {
            Arg::Num(Ok(pumpkin_plugin_api::command_wit::Number::Int32(n))) if n >= 0 => n as u32,
            Arg::Num(Ok(pumpkin_plugin_api::command_wit::Number::Int64(n))) if n >= 0 => n as u32,
            _ => {
                sender.send_message(TextComponent::text(
                    "Requirement must be a non-negative whole number of wins.",
                ));
                return Ok(0);
            }
        };

        if !self.tiers.set_requirement(&name, req) {
            sender.send_message(TextComponent::text(&format!(
                "No tier named '{name}'. Create it first with /tier \"{name}\"."
            )));
            return Ok(0);
        }
        self.tiers.save(&self.data_folder);
        sender.send_message(TextComponent::text(&format!(
            "Tier '{name}' now requires {req} wins."
        )));
        Ok(1)
    }
}

pub struct TierDeleteExecutor {
    pub tiers: Arc<TierRegistry>,
    pub data_folder: String,
}

impl CommandHandler for TierDeleteExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(name) = get_string_arg(&args, "name") else {
            sender.send_message(TextComponent::text("Usage: /tier \"<name>\" delete"));
            return Ok(0);
        };

        if !self.tiers.delete(&name) {
            sender.send_message(TextComponent::text(&format!("No tier named '{name}'.")));
            return Ok(0);
        }
        self.tiers.save(&self.data_folder);
        sender.send_message(TextComponent::text(&format!("Tier '{name}' deleted.")));
        Ok(1)
    }
}

pub struct TierListExecutor {
    pub tiers: Arc<TierRegistry>,
}

impl CommandHandler for TierListExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        let mut tiers = self.tiers.list();
        tiers.sort_by_key(|t| t.req);
        if tiers.is_empty() {
            sender.send_message(TextComponent::text("No tiers have been created yet."));
            return Ok(0);
        }
        let mut lines = vec!["Tiers (lowest to highest):".to_string()];
        for t in &tiers {
            lines.push(format!("- {} (requires {} wins)", t.name, t.req));
        }
        sender.send_message(TextComponent::text(&lines.join("\n")));
        Ok(1)
    }
}
