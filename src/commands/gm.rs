//! `/gm` — opens a generic 9x3 GUI listing every registered kit. Clicking a
//! kit's icon queues the clicker for that kit using the same shared queue
//! helper as `/queue`.

use std::sync::Arc;

use pumpkin_plugin_api::gui::{Gui, GuiType};
use pumpkin_plugin_api::item_stack::ItemStack;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{
    Server,
    command::{CommandError, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    uuid,
};

use crate::commands::queue_cmd::queue_player_for_kit;
use crate::kits::KitRegistry;
use crate::open_menu::{OpenMenu, OpenMenuRegistry};
use crate::queue::QueueManager;
use crate::state::{PlayerState, StateRegistry};

const GM_SLOTS_PER_ROW: u32 = 9;
const GM_ROWS: u32 = 3;
const GM_CAPACITY: usize = (GM_SLOTS_PER_ROW * GM_ROWS) as usize;

pub struct GmExecutor {
    pub kits: Arc<KitRegistry>,
    pub open_menus: Arc<OpenMenuRegistry>,
    pub state: Arc<StateRegistry>,
}

impl CommandHandler for GmExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            sender.send_message(TextComponent::text("Only players can open the kit menu."));
            return Ok(0);
        };

        // Queuing from the /gm menu goes through the same state gate as
        // /queue.
        match self.state.get_or_init(&uuid::to_string(player.get_id())) {
            PlayerState::Lobby => {}
            _ => {
                sender.send_message(TextComponent::text(
                    "You can't open the kit menu right now.",
                ));
                return Ok(0);
            }
        }

        let kits = self.kits.list();
        if kits.is_empty() {
            sender.send_message(TextComponent::text(
                "No kits have been created yet. An admin can make one with /lantern.",
            ));
            return Ok(0);
        }
        if kits.len() > GM_CAPACITY {
            tracing::warn!(
                "/gm has {} kits but only {GM_CAPACITY} slots fit in a generic-9x3 menu; \
                 the rest won't be shown. Consider a paged menu.",
                kits.len()
            );
        }

        let gui = Gui::new(GuiType::Generic9x3, TextComponent::text("Select a Kit"));
        for (slot, kit) in kits.iter().take(GM_CAPACITY).enumerate() {
            let icon = ItemStack::new(&kit.icon, 1);
            icon.set_custom_name(Some(TextComponent::text(&kit.name)));
            gui.set_item(slot as u32, icon);
        }
        gui.set_allow_grab_items(false);
        gui.set_allow_put_items(false);

        let uuid_str = uuid::to_string(player.get_id());
        self.open_menus.set(&uuid_str, OpenMenu::KitPicker);
        player.open_gui(gui);

        Ok(1)
    }
}

/// Shared by `/gm`'s click handler (registered once, globally, in lib.rs)
/// to resolve "slot N in the kit picker" -> "queue for this kit".
pub fn queue_for_kit_slot(
    slot: usize,
    kits: &KitRegistry,
    queue: &QueueManager,
    state: &StateRegistry,
    server: &Server,
    player: &pumpkin_plugin_api::player::Player,
) -> bool {
    let kit_list = kits.list();
    let Some(kit) = kit_list.get(slot) else {
        return false;
    };
    queue_player_for_kit(player, &kit.name, state, queue, server).is_ok()
}
