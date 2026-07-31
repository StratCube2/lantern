//! `/lantern <name>` — start editing a kit by clearing the player's
//! inventory and letting them place items directly, then save with `/done`.
//!
//! The kit contents are whatever the player puts into their inventory during
//! the build session. Existing kits are loaded into the player's inventory for
//! editing; new kits start from an empty inventory.
//!
//! `player.wit` exposes inventory access by a flat `slot: u8` index. This
//! code treats `0..36` as the editable hotbar + main inventory range.

use std::{collections::BTreeMap, sync::Arc};

use pumpkin_plugin_api::item_stack::ItemStack;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{
    Server,
    command::{Arg, CommandError, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    uuid,
};

use crate::kits::{Kit, KitItem, KitRegistry};
use crate::open_menu::{OpenMenu, OpenMenuRegistry};
use crate::state::{PlayerState, StateRegistry};

/// Number of real inventory slots we read back when saving a kit.
/// 0-8 hotbar, 9-35 main inventory. See the module doc's LIMITATION note.
const INVENTORY_SLOTS: u8 = 36;

pub struct LanternHelpExecutor {
    pub open_menus: Arc<OpenMenuRegistry>,
}

impl CommandHandler for LanternHelpExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            sender.send_message(TextComponent::text("Only players can create kits."));
            return Ok(0);
        };

        let uuid_str = uuid::to_string(player.get_id());
        match self.open_menus.get(&uuid_str) {
            Some(OpenMenu::KitCreatorEdit { kit_name }) if !kit_name.is_empty() => {
                sender.send_message(TextComponent::text(&format!(
                    "You're already editing kit '{}'. Use /done to save it or /cancel to discard it.",
                    kit_name
                )));
            }
            _ => {
                sender.send_message(TextComponent::text(
                    "Usage: /lantern <name>. Set up your inventory, then run /done to save it.",
                ));
            }
        }

        Ok(0)
    }
}

pub struct LanternStartExecutor {
    pub kits: Arc<KitRegistry>,
    pub open_menus: Arc<OpenMenuRegistry>,
    pub state: Arc<StateRegistry>,
}

impl CommandHandler for LanternStartExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            sender.send_message(TextComponent::text("Only players can create kits."));
            return Ok(0);
        };

        let uuid_str = uuid::to_string(player.get_id());
        if !matches!(self.state.get_or_init(&uuid_str), PlayerState::Lobby) {
            sender.send_message(TextComponent::text(
                "You can only edit kits while you're in the lobby.",
            ));
            return Ok(0);
        }

        if matches!(self.open_menus.get(&uuid_str), Some(OpenMenu::KitCreatorEdit { .. })) {
            sender.send_message(TextComponent::text(
                "You're already editing a kit. Use /done to save it or /cancel to discard it first.",
            ));
            return Ok(0);
        }

        let kit_name = match args.get_value("name") {
            Arg::Simple(s) => s,
            _ => {
                sender.send_message(TextComponent::text("Usage: /lantern <name>"));
                return Ok(0);
            }
        };

        clear_build_inventory(&player);

        if let Some(existing) = self.kits.get(&kit_name) {
            apply_kit_to_inventory(&player, &existing);
            sender.send_message(TextComponent::text(&format!(
                "Editing kit '{}'. Update your inventory, then run /done to save it.",
                kit_name
            )));
        } else {
            sender.send_message(TextComponent::text(&format!(
                "Creating kit '{}'. Fill your inventory, then run /done to save it.",
                kit_name
            )));
        }

        self.open_menus.set(&uuid_str, OpenMenu::KitCreatorEdit { kit_name });
        Ok(1)
    }
}

pub struct LanternDoneExecutor {
    pub kits: Arc<KitRegistry>,
    pub open_menus: Arc<OpenMenuRegistry>,
    pub data_folder: String,
}

impl CommandHandler for LanternDoneExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            sender.send_message(TextComponent::text("Only players can save a kit."));
            return Ok(0);
        };
        let uuid_str = uuid::to_string(player.get_id());

        let kit_name = match self.open_menus.get(&uuid_str) {
            Some(OpenMenu::KitCreatorEdit { kit_name }) if !kit_name.is_empty() => kit_name,
            _ => {
                sender.send_message(TextComponent::text(
                    "You're not currently editing a kit. Use /lantern <name> first.",
                ));
                return Ok(0);
            }
        };

        let items = collect_build_inventory(&player);
        let icon = items
            .values()
            .next()
            .map(|item| item.registry_key.clone())
            .unwrap_or_else(|| "minecraft:barrier".to_string());

        self.kits.upsert(Kit {
            name: kit_name.clone(),
            icon,
            items,
        });
        self.kits.save(&self.data_folder);
        self.open_menus.clear(&uuid_str);
        clear_build_inventory(&player);

        sender.send_message(TextComponent::text(&format!("Kit '{kit_name}' saved.")));
        Ok(1)
    }
}

fn clear_build_inventory(player: &pumpkin_plugin_api::player::Player) {
    for slot in 0..INVENTORY_SLOTS {
        player.set_inventory_item(slot, None);
    }
}

fn collect_build_inventory(player: &pumpkin_plugin_api::player::Player) -> BTreeMap<u8, KitItem> {
    let mut items = BTreeMap::new();
    for slot in 0..INVENTORY_SLOTS {
        if let Some(stack) = player.get_inventory_item(slot) {
            items.insert(
                slot,
                KitItem {
                    registry_key: stack.get_registry_key(),
                    count: stack.get_count(),
                },
            );
        }
    }
    items
}

fn apply_kit_to_inventory(player: &pumpkin_plugin_api::player::Player, kit: &Kit) {
    for (&slot, item) in &kit.items {
        if slot < INVENTORY_SLOTS {
            player.set_inventory_item(slot, Some(ItemStack::new(&item.registry_key, item.count)));
        }
    }
}

pub struct LanternCancelExecutor {
    pub open_menus: Arc<OpenMenuRegistry>,
}

impl CommandHandler for LanternCancelExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            return Ok(0);
        };
        let uuid_str = uuid::to_string(player.get_id());

        match self.open_menus.get(&uuid_str) {
            Some(OpenMenu::KitCreatorEdit { .. }) => {
                clear_build_inventory(&player);
                self.open_menus.clear(&uuid_str);
                sender.send_message(TextComponent::text("Kit editing cancelled."));
                Ok(1)
            }
            _ => {
                sender.send_message(TextComponent::text("You're not editing a kit."));
                Ok(0)
            }
        }
    }
}
