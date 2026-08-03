//! Lantern — a PvP practice core plugin for Pumpkin-MC.
//!
//! This is the wiring root: constructs every registry/manager, loads their
//! persisted state, registers permissions + command trees + event handlers,
//! and schedules the lobby HUD task. The actual feature logic lives in the
//! other modules (`arena`, `stats`, `tiers`, `matches`, `kits`, `queue`,
//! `state`, `lobby`, `open_menu`); this file only builds and connects them.

mod arena;
mod commands;
mod hud;
mod inventory_events;
mod kits;
mod lobby;
mod matches;
mod menu_router;
mod open_menu;
mod player_events;
mod queue;
mod state;
mod stats;
mod tiers;

use std::sync::Arc;

use pumpkin_plugin_api::events::EventPriority;
use pumpkin_plugin_api::permission::{Permission, PermissionChild, PermissionDefault, PermissionLevel};
use pumpkin_plugin_api::scheduler::SchedulerExt;
use pumpkin_plugin_api::{Context, Plugin, PluginMetadata, register_plugin};

use arena::ArenaRegistry;
use hud::{LOBBY_HUD_PERIOD_TICKS, tick_lobby_hud};
use inventory_events::{InventoryClickEvent, InventoryCloseEvent};
use kits::KitRegistry;
use lobby::{LobbyStore, TeleportPendingOnAction, TeleportToLobbyOnJoin};
use matches::MatchManager;
use menu_router::MenuClickRouter;
use open_menu::OpenMenuRegistry;
use player_events::{CleanupOnLeave, PlayerChatEvent, PlayerMoveEvent, SetStateOnJoin};
use pumpkin_plugin_api::events::player::{PlayerJoinEvent, PlayerLeaveEvent};
use queue::QueueManager;
use state::StateRegistry;
use stats::StatsRegistry;
use tiers::TierRegistry;

/// The Lantern plugin's permission node namespace. Must exactly match
/// `PluginMetadata.name` below, or the host rejects the plugin at load.
const PLUGIN_NAME: &str = "lantern";

fn perm_node(suffix: &str) -> String {
    format!("{PLUGIN_NAME}:{suffix}")
}

pub struct LanternPlugin {
    kits: Arc<KitRegistry>,
    arenas: Arc<ArenaRegistry>,
    stats: Arc<StatsRegistry>,
    tiers: Arc<TierRegistry>,
    state: Arc<StateRegistry>,
    queue: Arc<QueueManager>,
    lobby: Arc<LobbyStore>,
    open_menus: Arc<OpenMenuRegistry>,
    matches: Arc<MatchManager>,
    data_folder: String,
}

impl Plugin for LanternPlugin {
    fn new() -> Self {
        let kits = Arc::new(KitRegistry::new());
        let arenas = Arc::new(ArenaRegistry::new());
        let stats = Arc::new(StatsRegistry::new());
        let tiers = Arc::new(TierRegistry::new());
        let state = Arc::new(StateRegistry::new());
        let queue = Arc::new(QueueManager::new());
        let lobby = Arc::new(LobbyStore::new());
        let open_menus = Arc::new(OpenMenuRegistry::new());
        let matches = Arc::new(MatchManager::new(
            arenas.clone(),
            stats.clone(),
            tiers.clone(),
            state.clone(),
            lobby.clone(),
        ));

        Self {
            kits,
            arenas,
            stats,
            tiers,
            state,
            queue,
            lobby,
            open_menus,
            matches,
            data_folder: String::new(),
        }
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: PLUGIN_NAME.to_string(),
            version: "0.1.0".to_string(),
            authors: vec!["Joel".to_string()],
            description: "A practice PvP core: kits, arenas, matchmaking, stats, and tiers."
                .to_string(),
            dependencies: vec![],
            permissions: vec![
                pumpkin_plugin_api::permissions::FS_READ_DATA.to_string(),
                pumpkin_plugin_api::permissions::FS_WRITE_DATA.to_string(),
            ],
        }
    }

    fn on_load(&mut self, context: Context) -> pumpkin_plugin_api::Result<()> {
        self.data_folder = context.get_data_folder();

        // --- Load persisted state ---------------------------------------
        self.kits.load(&self.data_folder);
        self.arenas.load(&self.data_folder);
        self.stats.load(&self.data_folder);
        self.tiers.load(&self.data_folder);
        self.lobby.load(&self.data_folder);

        // --- Permissions --------------------------------------------------
        // Admin-only surfaces (arena editing, tier editing, kit creation)
        // default to op level three ("admin"); everything read-only
        // (queueing, browsing kits, checking stats) is open to everyone.
        register_permission(&context, "arena", "Manage practice arenas.", op_default())?;
        register_permission(&context, "tier", "Manage tier/level phases.", op_default())?;
        register_permission(&context, "lantern", "Create and manage kits.", op_default())?;
        register_permission(&context, "queue", "Queue for a kit.", PermissionDefault::Allow)?;
        register_permission(&context, "dequeue", "Leave a queue.", PermissionDefault::Allow)?;
        register_permission(&context, "gm", "Open the kit picker.", PermissionDefault::Allow)?;
        register_permission(&context, "stats", "View match statistics.", PermissionDefault::Allow)?;
        register_permission(
            &context,
            "leaderboard",
            "View top players.",
            PermissionDefault::Allow,
        )?;
        register_permission(&context, "done", "Save the kit you're editing.", op_default())?;
        register_permission(&context, "cancel", "Discard the kit you're editing.", op_default())?;
        register_permission(&context, "setlobby", "Set the lobby spawn point.", op_default())?;
        register_permission(&context, "duel", "Challenge a player directly.", PermissionDefault::Allow)?;

        // --- Command trees --------------------------------------------------
        let arena_tree = commands::build_arena_tree(self.arenas.clone(), self.data_folder.clone());
        context.register_command(arena_tree, &perm_node("arena"));

        let tier_tree = commands::build_tier_tree(self.tiers.clone(), self.data_folder.clone());
        context.register_command(tier_tree, &perm_node("tier"));

        let stats_tree = commands::build_stats_tree(self.stats.clone());
        context.register_command(stats_tree, &perm_node("stats"));

        let leaderboard_tree = commands::build_leaderboard_tree(self.stats.clone());
        context.register_command(leaderboard_tree, &perm_node("leaderboard"));

        let queue_tree = commands::build_queue_tree(
            self.kits.clone(),
            self.state.clone(),
            self.queue.clone(),
            self.matches.clone(),
        );
        context.register_command(queue_tree, &perm_node("queue"));

        let dequeue_cmd = commands::build_dequeue_command(self.state.clone(), self.queue.clone());
        context.register_command(dequeue_cmd, &perm_node("dequeue"));

        let gm_cmd = commands::build_gm_command(self.kits.clone(), self.open_menus.clone(), self.state.clone());
        context.register_command(gm_cmd, &perm_node("gm"));

        let lantern_tree = commands::build_lantern_tree(
            self.kits.clone(),
            self.open_menus.clone(),
            self.state.clone(),
            self.stats.clone(),
            self.data_folder.clone(),
        );
        context.register_command(lantern_tree, &perm_node("lantern"));

        let done_cmd =
            commands::build_done_command(self.kits.clone(), self.open_menus.clone(), self.data_folder.clone());
        context.register_command(done_cmd, &perm_node("done"));

        let cancel_cmd = commands::build_cancel_command(self.open_menus.clone());
        context.register_command(cancel_cmd, &perm_node("cancel"));

        let setlobby_cmd =
            commands::build_setlobby_command(self.lobby.clone(), self.data_folder.clone());
        context.register_command(setlobby_cmd, &perm_node("setlobby"));

        let duel_cmd = commands::build_duel_command();
        context.register_command(duel_cmd, &perm_node("duel"));

        // --- Event handlers -------------------------------------------------
        let menu_router = MenuClickRouter {
            kits: self.kits.clone(),
            queue: self.queue.clone(),
            state: self.state.clone(),
            open_menus: self.open_menus.clone(),
            matches: self.matches.clone(),
        };
        context.register_event_handler::<InventoryClickEvent, _>(
            menu_router,
            EventPriority::Normal,
            false,
        )?;

        let menu_router_close = MenuClickRouter {
            kits: self.kits.clone(),
            queue: self.queue.clone(),
            state: self.state.clone(),
            open_menus: self.open_menus.clone(),
            matches: self.matches.clone(),
        };
        context.register_event_handler::<InventoryCloseEvent, _>(
            menu_router_close,
            EventPriority::Normal,
            false,
        )?;

        let cleanup_on_leave = CleanupOnLeave {
            queue: self.queue.clone(),
            state: self.state.clone(),
            matches: self.matches.clone(),
            open_menus: self.open_menus.clone(),
            lobby: self.lobby.clone(),
        };
        context.register_event_handler::<PlayerLeaveEvent, _>(
            cleanup_on_leave,
            EventPriority::Normal,
            false,
        )?;

        // Lobby: mark state on join, then defer the actual teleport until
        // the player's first chat or move event (avoids racing the client's
        // own loading screen) -- see lobby.rs's module doc.
        let set_state_on_join = SetStateOnJoin {
            state: self.state.clone(),
        };
        context.register_event_handler::<PlayerJoinEvent, _>(set_state_on_join, EventPriority::Normal, false)?;

        let teleport_on_join = TeleportToLobbyOnJoin {
            store: self.lobby.clone(),
        };
        context.register_event_handler::<PlayerJoinEvent, _>(teleport_on_join, EventPriority::Normal, false)?;

        let teleport_pending_chat = TeleportPendingOnAction {
            store: self.lobby.clone(),
        };
        context.register_event_handler::<PlayerChatEvent, _>(
            teleport_pending_chat,
            EventPriority::Normal,
            false,
        )?;

        let teleport_pending_move = TeleportPendingOnAction {
            store: self.lobby.clone(),
        };
        context.register_event_handler::<PlayerMoveEvent, _>(
            teleport_pending_move,
            EventPriority::Normal,
            false,
        )?;

        // --- Lobby HUD --------------------------------------------------
        let hud_state = self.state.clone();
        let hud_stats = self.stats.clone();
        let hud_tiers = self.tiers.clone();
        context.schedule_repeating_task(LOBBY_HUD_PERIOD_TICKS, LOBBY_HUD_PERIOD_TICKS, move |server| {
            tick_lobby_hud(&server, &hud_state, &hud_stats, &hud_tiers);
        });

        tracing::info!("Lantern loaded.");
        Ok(())
    }

    fn on_unload(&mut self, _context: Context) -> pumpkin_plugin_api::Result<()> {
        self.kits.save(&self.data_folder);
        self.arenas.save(&self.data_folder);
        self.stats.save(&self.data_folder);
        self.tiers.save(&self.data_folder);
        self.lobby.save(&self.data_folder);
        tracing::info!("Lantern unloaded.");
        Ok(())
    }
}

fn op_default() -> PermissionDefault {
    PermissionDefault::Op(PermissionLevel::Three)
}

fn register_permission(
    context: &Context,
    suffix: &str,
    description: &str,
    default: PermissionDefault,
) -> pumpkin_plugin_api::Result<()> {
    let permission = Permission {
        node: perm_node(suffix),
        description: description.to_string(),
        default,
        children: Vec::<PermissionChild>::new(),
    };
    context.register_permission(&permission).map_err(|e| e.to_string())
}

register_plugin!(LanternPlugin);
