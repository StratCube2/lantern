//! Builds every command tree Lantern registers.
//!
//! `CommandNode::then()`/`Command::then()` mutate the node in place and
//! return `()` (see `command.wit`'s `resource command-node`), so none of
//! this can be written as fluent chaining — every subtree has to be built
//! bottom-up into a local `let` binding, attached to its parent with a bare
//! `.then(...)` statement, and only the finished root handed back.
//!
//! Each `build_*_tree` function returns a ready-to-register `Command`; the
//! caller (`lib.rs::on_load`) is responsible for calling
//! `context.register_command(cmd, "<permission-node>")` with a namespaced
//! permission such as `lantern:arena`.

pub mod arena_cmd;
pub mod duel;
pub mod gm;
pub mod lantern;
pub mod queue_cmd;
pub mod stats_cmd;
pub mod tier_cmd;

use std::sync::Arc;

use pumpkin_plugin_api::command::{ArgumentType, Command, CommandNode, StringType};

use crate::arena::ArenaRegistry;
use crate::kits::KitRegistry;
use crate::matches::MatchManager;
use crate::open_menu::OpenMenuRegistry;
use crate::queue::QueueManager;
use crate::state::StateRegistry;
use crate::stats::{LeaderboardSort, StatsRegistry};
use crate::tiers::TierRegistry;

use duel::DuelChallengeExecutor;
use arena_cmd::{
    ArenaCreateExecutor, ArenaDeleteExecutor, ArenaListExecutor, ArenaRenameExecutor,
    ArenaSetPosAExecutor, ArenaSetPosBExecutor,
};
use gm::GmExecutor;
use lantern::{
    LanternCancelExecutor, LanternDoneExecutor, LanternHelpExecutor, LanternKitCountdownExecutor,
    LanternKitDeleteExecutor, LanternKitFallDamageExecutor, LanternKitMultiplayerExecutor,
    LanternKitRenameExecutor, LanternStartExecutor, LanternStatsResetExecutor,
};
use queue_cmd::{QueueHelpExecutor, QueueKitExecutor, QueueLeaveExecutor};
use stats_cmd::{LeaderboardCategoryExecutor, LeaderboardExecutor, StatsOtherExecutor, StatsSelfExecutor};
use tier_cmd::{TierCreateExecutor, TierDeleteExecutor, TierListExecutor, TierReqExecutor};

fn name_arg() -> CommandNode {
    CommandNode::argument("name", &ArgumentType::String(StringType::Quotable))
}

fn player_arg(key: &str) -> CommandNode {
    CommandNode::argument(key, &ArgumentType::String(StringType::SingleWord))
}

fn bool_arg(key: &str) -> CommandNode {
    CommandNode::argument(key, &ArgumentType::Bool)
}

/// `/arena create|delete|rename|setposa|setposb|list`
pub fn build_arena_tree(arenas: Arc<ArenaRegistry>, data_folder: String) -> Command {
    let cmd = Command::new(&["arena".to_string()], "Manage practice arenas.");

    let create_name = name_arg().execute(ArenaCreateExecutor {
        arenas: arenas.clone(),
        data_folder: data_folder.clone(),
    });
    let create = CommandNode::literal("create");
    create.then(create_name);

    let delete_name = name_arg().execute(ArenaDeleteExecutor {
        arenas: arenas.clone(),
        data_folder: data_folder.clone(),
    });
    let delete = CommandNode::literal("delete");
    delete.then(delete_name);

    let rename_new_name = CommandNode::argument("new_name", &ArgumentType::String(StringType::Quotable))
        .execute(ArenaRenameExecutor {
            arenas: arenas.clone(),
            data_folder: data_folder.clone(),
        });
    let rename_name = name_arg();
    rename_name.then(rename_new_name);
    let rename = CommandNode::literal("rename");
    rename.then(rename_name);

    let setposa_name = name_arg().execute(ArenaSetPosAExecutor {
        arenas: arenas.clone(),
        data_folder: data_folder.clone(),
    });
    let setposa = CommandNode::literal("setposa");
    setposa.then(setposa_name);

    let setposb_name = name_arg().execute(ArenaSetPosBExecutor {
        arenas: arenas.clone(),
        data_folder: data_folder.clone(),
    });
    let setposb = CommandNode::literal("setposb");
    setposb.then(setposb_name);

    let list = CommandNode::literal("list").execute(ArenaListExecutor { arenas: arenas.clone() });

    cmd.then(create);
    cmd.then(delete);
    cmd.then(rename);
    cmd.then(setposa);
    cmd.then(setposb);
    cmd.then(list);

    cmd
}

/// `/tier "<name>"` (create), `/tier "<name>" req <wins>`,
/// `/tier "<name>" delete`, `/tier list`.
pub fn build_tier_tree(tiers: Arc<TierRegistry>, data_folder: String) -> Command {
    let cmd = Command::new(&["tier".to_string()], "Manage tier/level phases.");

    let req_value = CommandNode::argument("req", &ArgumentType::Integer((Some(0), None))).execute(
        TierReqExecutor {
            tiers: tiers.clone(),
            data_folder: data_folder.clone(),
        },
    );
    let req = CommandNode::literal("req");
    req.then(req_value);

    let delete = CommandNode::literal("delete").execute(TierDeleteExecutor {
        tiers: tiers.clone(),
        data_folder: data_folder.clone(),
    });

    // `/tier "<name>"` on its own creates the tier; `req`/`delete` are
    // sibling literals hung off the same name argument so
    // `/tier "<name>" req <wins>` and `/tier "<name>" delete` both work
    // after the name has been consumed.
    let name = name_arg().execute(TierCreateExecutor {
        tiers: tiers.clone(),
        data_folder: data_folder.clone(),
    });
    name.then(req);
    name.then(delete);
    cmd.then(name);

    let list = CommandNode::literal("list").execute(TierListExecutor { tiers: tiers.clone() });
    cmd.then(list);

    cmd
}

/// `/stats` (self), `/stats <player>`.
pub fn build_stats_tree(stats: Arc<StatsRegistry>) -> Command {
    let cmd = Command::new(&["stats".to_string()], "View match statistics.")
        .execute(StatsSelfExecutor { stats: stats.clone() });

    let player = player_arg("player").execute(StatsOtherExecutor { stats: stats.clone() });
    cmd.then(player);

    // `/stats reset <player>` is intentionally not a subcommand here --
    // resetting stats is an admin action and lives under the
    // already-admin-gated `/lantern stats reset <player>` instead (see
    // `build_lantern_tree`), rather than needing its own separate
    // permission split under the open `lantern:stats` node.
    cmd
}

/// `/leaderboard` (defaults to wins), `/leaderboard <category>`.
pub fn build_leaderboard_tree(stats: Arc<StatsRegistry>) -> Command {
    let cmd = Command::new(&["leaderboard".to_string()], "View top players.").execute(
        LeaderboardExecutor {
            stats: stats.clone(),
            default_sort: LeaderboardSort::Wins,
        },
    );

    let category = player_arg("category").execute(LeaderboardCategoryExecutor { stats: stats.clone() });
    cmd.then(category);

    cmd
}

/// `/queue <kit>`, `/dequeue`.
pub fn build_queue_tree(
    kits: Arc<KitRegistry>,
    state: Arc<StateRegistry>,
    queue: Arc<QueueManager>,
    matches: Arc<MatchManager>,
) -> Command {
    let cmd = Command::new(&["queue".to_string()], "Queue for a kit.").execute(
        QueueHelpExecutor {
            message: "Usage: /queue <kit>. Use /gm to browse available kits.".to_string(),
        },
    );

    let kit = player_arg("kit").execute(QueueKitExecutor {
        kits: kits.clone(),
        state: state.clone(),
        queue: queue.clone(),
        matches: matches.clone(),
    });
    cmd.then(kit);

    cmd
}

pub fn build_dequeue_command(state: Arc<StateRegistry>, queue: Arc<QueueManager>) -> Command {
    Command::new(&["dequeue".to_string()], "Leave your current queue.")
        .execute(QueueLeaveExecutor { state, queue })
}

pub fn build_gm_command(
    kits: Arc<KitRegistry>,
    open_menus: Arc<OpenMenuRegistry>,
    state: Arc<StateRegistry>,
) -> Command {
    Command::new(&["gm".to_string()], "Open the kit picker.").execute(GmExecutor {
        kits,
        open_menus,
        state,
    })
}

/// `/lantern <name>` (create/edit), `/lantern kit "<name>" multiplayer|
/// falldamage|countdown <true|false>`, `/lantern "<name>" delete|rename
/// <new_name>`, `/lantern stats reset <player>`. `/done`/`/cancel` are
/// registered separately (see `build_done_command`/`build_cancel_command`)
/// since they're their own top-level commands, not subcommands of
/// `/lantern`.
pub fn build_lantern_tree(
    kits: Arc<KitRegistry>,
    open_menus: Arc<OpenMenuRegistry>,
    state: Arc<StateRegistry>,
    stats: Arc<StatsRegistry>,
    data_folder: String,
) -> Command {
    let cmd = Command::new(&["lantern".to_string()], "Create and manage kits.").execute(
        LanternHelpExecutor {
            open_menus: open_menus.clone(),
        },
    );

    // `/lantern kit "<name>" multiplayer|falldamage|countdown <true|false>`
    let mp_value = bool_arg("value").execute(LanternKitMultiplayerExecutor {
        kits: kits.clone(),
        data_folder: data_folder.clone(),
    });
    let mp = CommandNode::literal("multiplayer");
    mp.then(mp_value);

    let fd_value = bool_arg("value").execute(LanternKitFallDamageExecutor {
        kits: kits.clone(),
        data_folder: data_folder.clone(),
    });
    let fd = CommandNode::literal("falldamage");
    fd.then(fd_value);

    let cd_value = bool_arg("value").execute(LanternKitCountdownExecutor {
        kits: kits.clone(),
        data_folder: data_folder.clone(),
    });
    let cd = CommandNode::literal("countdown");
    cd.then(cd_value);

    let kit_settings_name = name_arg();
    kit_settings_name.then(mp);
    kit_settings_name.then(fd);
    kit_settings_name.then(cd);
    let kit_lit = CommandNode::literal("kit");
    kit_lit.then(kit_settings_name);
    cmd.then(kit_lit);

    // `/lantern stats reset <player>`
    let reset_player = player_arg("player").execute(LanternStatsResetExecutor {
        stats: stats.clone(),
        data_folder: data_folder.clone(),
    });
    let reset = CommandNode::literal("reset");
    reset.then(reset_player);
    let stats_lit = CommandNode::literal("stats");
    stats_lit.then(reset);
    cmd.then(stats_lit);

    // `/lantern "<name>"` (start/edit build session), with `delete`/
    // `rename <new_name>` as sibling literals off the same name argument --
    // matches the `/tier "<name>" req|delete` shape above.
    let delete = CommandNode::literal("delete").execute(LanternKitDeleteExecutor {
        kits: kits.clone(),
        data_folder: data_folder.clone(),
    });
    let rename_new_name = CommandNode::argument("new_name", &ArgumentType::String(StringType::Quotable))
        .execute(LanternKitRenameExecutor {
            kits: kits.clone(),
            data_folder: data_folder.clone(),
        });
    let rename = CommandNode::literal("rename");
    rename.then(rename_new_name);

    let name = name_arg().execute(LanternStartExecutor {
        kits: kits.clone(),
        open_menus: open_menus.clone(),
        state: state.clone(),
    });
    name.then(delete);
    name.then(rename);
    cmd.then(name);

    cmd
}

pub fn build_done_command(kits: Arc<KitRegistry>, open_menus: Arc<OpenMenuRegistry>, data_folder: String) -> Command {
    Command::new(&["done".to_string()], "Save the kit you're editing.").execute(
        LanternDoneExecutor {
            kits,
            open_menus,
            data_folder,
        },
    )
}

pub fn build_cancel_command(open_menus: Arc<OpenMenuRegistry>) -> Command {
    Command::new(&["cancel".to_string()], "Discard the kit you're editing.")
        .execute(LanternCancelExecutor { open_menus })
}

/// `/setlobby` — the executor and `LobbyStore` it wraps live in `lobby.rs`
/// (deferred-teleport design; see that module's doc comment), but the
/// command tree is built here for consistency with every other tree in this
/// file.
pub fn build_setlobby_command(store: Arc<crate::lobby::LobbyStore>, data_folder: String) -> Command {
    Command::new(&["setlobby".to_string()], "Set the lobby spawn point to your current position.")
        .execute(crate::lobby::SetLobbyExecutor { store, data_folder })
}

/// `/duel <player>` — challenge a specific player directly. Stub only; see
/// `commands/duel.rs`'s module doc for what's still unimplemented
/// (challenge/accept flow, timeout, which kit a direct duel uses).
pub fn build_duel_command() -> Command {
    let cmd = Command::new(&["duel".to_string()], "Challenge a specific player to a duel.");

    let target = CommandNode::argument("target", &ArgumentType::Players).execute(DuelChallengeExecutor);
    cmd.then(target);

    cmd
}
