//! Tracks which of *our* menus (if any) a given player currently has open.
//!
//! The host's `InventoryClickEvent`/`InventoryCloseEvent` (see
//! `inventory_events.rs`) tell us *that* a player clicked or closed
//! something, but not which logical menu it was — there's no host-provided
//! "gui instance id" on the event data to compare against the `Gui` we
//! opened (`gui.wit`'s `inventory-click-event-data` only carries a
//! `window-type: option<string>`, not a handle back to our `Gui` resource).
//! So `menu_router.rs` looks the clicking player up here instead, keyed by
//! uuid, to decide how to interpret their click.
//!
//! `KitCreatorEdit` isn't actually a GUI (it's a real-inventory build
//! session started by `/lantern <n>`), but it's tracked here too since it's
//! the other "player is mid-flow with our system" state that needs to gate
//! re-entrancy (e.g. stop `/gm` from being opened mid-kit-edit).

use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenMenu {
    /// The `/gm` kit picker GUI.
    KitPicker,
    /// A `/lantern <n>` build session in progress; the player's real
    /// inventory is standing in for the kit contents until `/done`.
    KitCreatorEdit { kit_name: String },
}

pub struct OpenMenuRegistry {
    inner: RwLock<HashMap<String, OpenMenu>>,
}

impl OpenMenuRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    pub fn get(&self, uuid: &str) -> Option<OpenMenu> {
        self.inner.read().unwrap().get(uuid).cloned()
    }

    pub fn set(&self, uuid: &str, menu: OpenMenu) {
        self.inner.write().unwrap().insert(uuid.to_string(), menu);
    }

    pub fn clear(&self, uuid: &str) {
        self.inner.write().unwrap().remove(uuid);
    }
}

impl Default for OpenMenuRegistry {
    fn default() -> Self {
        Self::new()
    }
}
