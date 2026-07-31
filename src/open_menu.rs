//! Per-player "which of our GUIs is open right now" registry.
//!
//! `inventory-click-event-data` (see `event.wit`) gives us `window-type:
//! option<string>` — that's the container *shape* (a generic 9x3 chest,
//! etc.), not a handle back to the specific `Gui` resource we opened. There
//! is no sync-id or GUI-instance identifier exposed to plugins anywhere in
//! the WIT surface (checked `gui.wit`, `event.wit`, `player.wit`).
//!
//! So: every time we call `player.open_gui(...)`, we also record what *we*
//! think is now open for that player here. When a click event fires, we look
//! the player up and route based on that, instead of trying to infer intent
//! from `window-type` alone. `InventoryCloseEvent` (and overwriting with a
//! new `open_gui` call) clears/replaces the entry.

use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenMenu {
    KitCreatorEdit { kit_name: String },
    /// /gm kit picker.
    KitPicker,
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

    pub fn set(&self, uuid: &str, menu: OpenMenu) {
        self.inner.write().unwrap().insert(uuid.to_string(), menu);
    }

    pub fn get(&self, uuid: &str) -> Option<OpenMenu> {
        self.inner.read().unwrap().get(uuid).cloned()
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
