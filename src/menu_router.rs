//! Routes `InventoryClickEvent`/`InventoryCloseEvent` to whichever of our
//! GUIs (if any) the clicking player currently has open, per
//! `open_menu::OpenMenuRegistry`. See that module's doc comment for why this
//! indirection exists (no host-provided way to identify which `Gui`
//! instance a click belongs to).

use std::sync::Arc;

use pumpkin_plugin_api::events::EventHandler;
use pumpkin_plugin_api::uuid;

use crate::commands::gm::queue_for_kit_slot;
use crate::inventory_events::{
    InventoryClickEvent, InventoryCloseEvent, InventoryClickEventData, InventoryCloseEventData,
};
use crate::kits::KitRegistry;
use crate::matches::MatchManager;
use crate::open_menu::{OpenMenu, OpenMenuRegistry};
use crate::queue::QueueManager;
use crate::state::StateRegistry;

pub struct MenuClickRouter {
    pub kits: Arc<KitRegistry>,
    pub queue: Arc<QueueManager>,
    pub state: Arc<StateRegistry>,
    pub open_menus: Arc<OpenMenuRegistry>,
    pub matches: Arc<MatchManager>,
}

impl EventHandler<InventoryClickEvent> for MenuClickRouter {
    fn handle(
        &self,
        server: pumpkin_plugin_api::Server,
        mut data: InventoryClickEventData,
    ) -> InventoryClickEventData {
        let uuid_str = uuid::to_string(data.player.get_id());
        let Some(menu) = self.open_menus.get(&uuid_str) else {
            // Not one of our menus (or the player has nothing of ours open)
            // -- leave the event alone entirely.
            return data;
        };

        // Every menu we open is click-locked (set_allow_grab_items/put_items
        // = false), so any click inside it should never move an item. Cancel
        // first, then run our own routing logic off the slot that was
        // clicked.
        data.cancelled = true;

        if data.slot < 0 {
            return data;
        }
        let slot = data.slot as usize;

        match menu {
            OpenMenu::KitPicker => {
                let _ = queue_for_kit_slot(
                    slot,
                    &self.kits,
                    &self.queue,
                    &self.state,
                    &self.matches,
                    &server,
                    &data.player,
                );
                self.open_menus.clear(&uuid_str);
            }
            OpenMenu::KitCreatorEdit { .. } => {
                // Player is mid-build-session (editing their real inventory,
                // not a GUI) -- clicks shouldn't route here since the GUI is
                // already closed by this point, but guard anyway.
            }
        }

        data
    }
}

impl EventHandler<InventoryCloseEvent> for MenuClickRouter {
    fn handle(
        &self,
        _server: pumpkin_plugin_api::Server,
        data: InventoryCloseEventData,
    ) -> InventoryCloseEventData {
        let uuid_str = uuid::to_string(data.player.get_id());
        // Only clear the kit picker on close -- an active build session
        // (KitCreatorEdit) isn't a GUI at all.
        if matches!(self.open_menus.get(&uuid_str), Some(OpenMenu::KitPicker)) {
            self.open_menus.clear(&uuid_str);
        }
        data
    }
}
