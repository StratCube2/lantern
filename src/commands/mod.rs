pub mod duel;
pub mod gm;
pub mod lantern;
pub mod queue_cmd;

use std::sync::Arc;

use pumpkin_plugin_api::command::{ArgumentType, Command, CommandNode, StringType};

use crate::kits::KitRegistry;
use crate::lobby::LobbyStore;
use crate::open_menu::OpenMenuRegistry;
use crate::queue::QueueManager;
use crate::state::StateRegistry;
use duel::DuelChallengeExecutor;
use gm::GmExecutor;
use lantern::{LanternCancelExecutor, LanternDoneExecutor, LanternHelpExecutor, LanternStartExecutor};
use queue_cmd::{QueueHelpExecutor, QueueKitExecutor, QueueLeaveExecutor};

// then() mutates the node/command in place and returns nothing -- it is not
// a builder chain by itself (confirmed against command.wit: `then: func
// (node: command-node)`, no return value). Only `.execute(...)` (a
// hand-written wrapper, not raw WIT) is genuinely chainable. Every tree
// below is built by declaring child nodes first, then attaching with bare
// `.then(...)` calls, same as the original /duel tree.

/// Builds the `/queue <kit>` tree.
pub fn build_queue_tree(
    kits: Arc<KitRegistry>,
    state: Arc<StateRegistry>,
    queue: Arc<QueueManager>,
) -> Command {
    let names = ["queue".to_string()];
    let command = Command::new(&names, "Queue for a named kit.");

    let root = command.execute(QueueHelpExecutor {
        message: "Usage: /queue <kit>. Use /gm to browse kits.".to_string(),
    });

    let kit_arg = CommandNode::argument("kit", &ArgumentType::String(StringType::SingleWord));
    let kit_arg = kit_arg.execute(QueueKitExecutor {
        kits,
        state,
        queue,
    });
    root.then(kit_arg);

    root
}

/// Builds the `/dequeue` tree.
pub fn build_dequeue_tree(state: Arc<StateRegistry>, queue: Arc<QueueManager>) -> Command {
    let names = ["dequeue".to_string()];
    let command = Command::new(&names, "Leave whatever kit queue you're in.");
    command.execute(QueueLeaveExecutor { state, queue })
}

/// Builds the `/duel <player>` stub tree.
pub fn build_duel_tree() -> Command {
    let names = ["duel".to_string()];
    let command = Command::new(&names, "Challenge a specific player to a duel.");

    let target_node = CommandNode::argument("target", &ArgumentType::Players);
    let target_node = target_node.execute(DuelChallengeExecutor);
    command.then(target_node);

    command
}

/// Builds the `/lantern <name>` kit editor.
pub fn build_lantern_tree(
    kits: Arc<KitRegistry>,
    open_menus: Arc<OpenMenuRegistry>,
    state: Arc<StateRegistry>,
) -> Command {
    let names = ["lantern".to_string()];
    let command = Command::new(&names, "Create or edit a kit.");
    let command = command.execute(LanternHelpExecutor {
        open_menus: open_menus.clone(),
    });

    let name_arg = CommandNode::argument("name", &ArgumentType::String(StringType::SingleWord));
    let name_arg = name_arg.execute(LanternStartExecutor {
        kits: kits.clone(),
        open_menus: open_menus.clone(),
        state,
    });
    command.then(name_arg);

    command
}

/// Builds the `/done` tree.
pub fn build_done_tree(
    kits: Arc<KitRegistry>,
    open_menus: Arc<OpenMenuRegistry>,
    data_folder: String,
) -> Command {
    let names = ["done".to_string()];
    let command = Command::new(&names, "Save the kit you're currently editing.");
    command.execute(LanternDoneExecutor {
        kits,
        open_menus,
        data_folder,
    })
}

/// Builds the `/cancel` tree.
pub fn build_cancel_tree(open_menus: Arc<OpenMenuRegistry>) -> Command {
    let names = ["cancel".to_string()];
    let command = Command::new(&names, "Cancel the kit editor.");
    command.execute(LanternCancelExecutor { open_menus })
}

/// Builds the `/gm` tree — a single command with no subcommands.
pub fn build_gm_tree(
    kits: Arc<KitRegistry>,
    open_menus: Arc<OpenMenuRegistry>,
    state: Arc<StateRegistry>,
) -> Command {
    let names = ["gm".to_string()];
    let command = Command::new(&names, "Open the kit queue menu.");
    command.execute(GmExecutor {
        kits,
        open_menus,
        state,
    })
}

/// Builds the `/setlobby` tree — a single command with no subcommands.
pub fn build_setlobby_tree(store: Arc<LobbyStore>, data_folder: String) -> Command {
    let names = ["setlobby".to_string()];
    let command = Command::new(&names, "Set the lobby spawn point to your current position.");
    command.execute(crate::lobby::SetLobbyExecutor { store, data_folder })
}
