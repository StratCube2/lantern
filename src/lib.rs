mod commands;
mod inventory_events;
mod kits;
mod lobby;
mod menu_router;
mod open_menu;
mod player_events;
mod queue;
mod state;

use std::sync::Arc;

use pumpkin_plugin_api::{
    Context, Plugin, PluginMetadata,
    events::EventPriority,
    permission::{Permission, PermissionDefault, PermissionLevel},
    permissions,
    scheduler::SchedulerExt,
    uuid,
};
use tracing::*;

use inventory_events::{InventoryClickEvent, InventoryCloseEvent};
use kits::KitRegistry;
use lobby::LobbyStore;
use menu_router::MenuClickRouter;
use open_menu::OpenMenuRegistry;
use pumpkin_plugin_api::events::player::PlayerLeaveEvent;
use pumpkin_plugin_api::events::{EventHandler, FromIntoEvent};
use queue::QueueManager;
use state::{PlayerState, StateRegistry};

const PERM_QUEUE: &str = "LanternPractice:command.queue";
const PERM_DUEL: &str = "LanternPractice:command.duel";
const PERM_GM: &str = "LanternPractice:command.gm";
const PERM_LANTERN: &str = "LanternPractice:command.lantern";
const PERM_SETLOBBY: &str = "LanternPractice:command.setlobby";

struct LanternPracticePlugin {
    state: Arc<StateRegistry>,
    queue: Arc<QueueManager>,
    kits: Arc<KitRegistry>,
    open_menus: Arc<OpenMenuRegistry>,
    lobby: Arc<LobbyStore>,
}

impl Plugin for LanternPracticePlugin {
    fn new() -> Self {
        LanternPracticePlugin {
            state: Arc::new(StateRegistry::new()),
            queue: Arc::new(QueueManager::new()),
            kits: Arc::new(KitRegistry::new()),
            open_menus: Arc::new(OpenMenuRegistry::new()),
            lobby: Arc::new(LobbyStore::new()),
        }
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "LanternPractice".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["you".into()],
            description: "1v1/XvX practice duels for Pumpkin.".into(),
            dependencies: vec![],
            // fs.read.data / fs.write.data are required for lobby.toml and
            // kits.toml persistence in the plugin's data folder (see
            // permissions.rs doc comments on these two constants).
            permissions: vec![permissions::FS_READ_DATA.into(), permissions::FS_WRITE_DATA.into()],
        }
    }

    fn on_load(&mut self, context: Context) -> pumpkin_plugin_api::Result<()> {
        info!("LanternPractice loading...");

        let data_folder = context.get_data_folder();

        // -- Permissions --------------------------------------------------
        // /queue, /dequeue, /duel, /gm are open to everyone by default; /lantern
        // and /setlobby default to gamemaster-tier (level 2) since they let
        // someone reshape the lobby / create kits server-wide.
        context.register_permission(&Permission {
            node: PERM_QUEUE.to_string(),
            description: "Allows queueing for a Practice duel".to_string(),
            default: PermissionDefault::Allow,
            children: Vec::new(),
        })?;
        context.register_permission(&Permission {
            node: PERM_DUEL.to_string(),
            description: "Allows challenging another player directly".to_string(),
            default: PermissionDefault::Allow,
            children: Vec::new(),
        })?;
        context.register_permission(&Permission {
            node: PERM_GM.to_string(),
            description: "Allows opening the kit queue menu".to_string(),
            default: PermissionDefault::Allow,
            children: Vec::new(),
        })?;
        context.register_permission(&Permission {
            node: PERM_LANTERN.to_string(),
            description: "Allows creating/editing kits".to_string(),
            default: PermissionDefault::Op(PermissionLevel::Two),
            children: Vec::new(),
        })?;
        context.register_permission(&Permission {
            node: PERM_SETLOBBY.to_string(),
            description: "Allows setting the lobby spawn point".to_string(),
            default: PermissionDefault::Op(PermissionLevel::Two),
            children: Vec::new(),
        })?;

        // -- Commands -------------------------------------------------------
        let queue_tree = commands::build_queue_tree(
            self.kits.clone(),
            self.state.clone(),
            self.queue.clone(),
        );
        context.register_command(queue_tree, PERM_QUEUE);

        let dequeue_tree = commands::build_dequeue_tree(self.state.clone(), self.queue.clone());
        context.register_command(dequeue_tree, PERM_QUEUE);

        let duel_tree = commands::build_duel_tree();
        context.register_command(duel_tree, PERM_DUEL);

        let gm_tree = commands::build_gm_tree(
            self.kits.clone(),
            self.open_menus.clone(),
            self.state.clone(),
        );
        context.register_command(gm_tree, PERM_GM);

        let lantern_tree = commands::build_lantern_tree(
            self.kits.clone(),
            self.open_menus.clone(),
            self.state.clone(),
        );
        context.register_command(lantern_tree, PERM_LANTERN);

        let done_tree = commands::build_done_tree(
            self.kits.clone(),
            self.open_menus.clone(),
            data_folder.clone(),
        );
        context.register_command(done_tree, PERM_LANTERN);

        let cancel_tree = commands::build_cancel_tree(self.open_menus.clone());
        context.register_command(cancel_tree, PERM_LANTERN);

        let setlobby_tree = commands::build_setlobby_tree(self.lobby.clone(), data_folder.clone());
        context.register_command(setlobby_tree, PERM_SETLOBBY);

        // -- Persistence: load kits + lobby from disk ------------------------
        self.kits.load(&data_folder);
        lobby::register(&context, self.lobby.clone(), data_folder.clone())?;

        // -- Events -----------------------------------------------------------
        context.register_event_handler::<InventoryClickEvent, _>(
            MenuClickRouter {
                kits: self.kits.clone(),
                queue: self.queue.clone(),
                state: self.state.clone(),
                open_menus: self.open_menus.clone(),
            },
            EventPriority::Normal,
            true, // blocking: we set `cancelled = true` ourselves and need that to stick
        )?;
        context.register_event_handler::<InventoryCloseEvent, _>(
            MenuClickRouter {
                kits: self.kits.clone(),
                queue: self.queue.clone(),
                state: self.state.clone(),
                open_menus: self.open_menus.clone(),
            },
            EventPriority::Normal,
            false,
        )?;
        context.register_event_handler::<PlayerLeaveEvent, _>(
            CleanupOnLeave {
                state: self.state.clone(),
                queue: self.queue.clone(),
                open_menus: self.open_menus.clone(),
                lobby: self.lobby.clone(),
            },
            EventPriority::Normal,
            false,
        )?;

        // -- Periodic state reconciliation (unchanged from Phase 1-4) --------
        let state_for_task = self.state.clone();
        let queue_for_task = self.queue.clone();
        context.schedule_repeating_task(600, 600, move |server| {
            let online: Vec<String> = server
                .get_all_players()
                .into_iter()
                .map(|p| uuid::to_string(p.get_id()))
                .collect();
            state_for_task.reconcile(&online);
            let _ = &queue_for_task;
        });

        info!(
            "LanternPractice loaded. /queue, /dequeue, /duel <player>, /gm, /lantern <name>, /done, /setlobby"
        );
        Ok(())
    }

    fn on_unload(&mut self, _context: Context) -> pumpkin_plugin_api::Result<()> {
        info!("LanternPractice unloaded. Goodbye!");
        Ok(())
    }
}

pumpkin_plugin_api::register_plugin!(LanternPracticePlugin);

/// Clears a leaving player's tracked state and, if they were queued, pulls
/// them out of whatever queue pool they were sitting in so they don't sit
/// there forever after disconnecting.
struct CleanupOnLeave {
    state: Arc<StateRegistry>,
    queue: Arc<QueueManager>,
    open_menus: Arc<OpenMenuRegistry>,
    lobby: Arc<LobbyStore>,
}

impl EventHandler<PlayerLeaveEvent> for CleanupOnLeave {
    fn handle(
        &self,
        _server: pumpkin_plugin_api::Server,
        data: <PlayerLeaveEvent as FromIntoEvent>::Data,
    ) -> <PlayerLeaveEvent as FromIntoEvent>::Data {
        let uuid_str = uuid::to_string(data.player.get_id());
        if let PlayerState::Queued { mode } = self.state.get_or_init(&uuid_str) {
            self.queue.dequeue(mode, &uuid_str);
        }
        self.state.remove(&uuid_str);
        self.open_menus.clear(&uuid_str);
        self.lobby.clear_pending(&uuid_str);
        data
    }
}
