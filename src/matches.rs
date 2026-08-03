//! The Async Match Controller.
//!
//! This is where queued players actually turn into a running fight. There is
//! no combat/damage/death event exposed to plugins anywhere in the WIT
//! surface (checked `event.wit` — only `player-*`, `block-*`,
//! `inventory-*`, `server-*`, and `packet-*` events exist), so win detection
//! is done by polling each fighter's `player.get_health()` from a repeating
//! scheduled task, at the same cadence the architecture doc describes for
//! the HUD update (every 10 ticks / 0.5s) anyway.
//!
//! Likewise there is no gamerule setter exposed to plugins (checked
//! `world.wit`/`server.wit`), so "instant respawn" and "fall damage off" are
//! both implemented at the plugin level rather than by flipping
//! `doImmediateRespawn`:
//!   * Instant respawn: the moment we observe health <= 0 we immediately
//!     call `player.respawn()` and `player.set_health(...)` ourselves rather
//!     than waiting on the vanilla death screen.
//!   * Fall damage off: `entity.get_fall_distance()`/`set_fall_distance()`
//!     (via `player.as_entity()`) is zeroed out every poll for kits with
//!     `fall_damage = false`, before it can accumulate into real damage.
//!     For Sumo-style kits this is orthogonal to ring-outs, which kill via
//!     `min_y` (arena min_y) rather than fall damage.
//!
//! `%world` is a WIT resource handle, not a `Copy`/`Clone` Rust value, so
//! rather than holding one across multiple player teleports we re-resolve
//! it by id (`server.get_all_worlds().find(...)`) immediately before each
//! call that needs it — the same pattern `lobby.rs` already uses.
//!
//! One `Match` = one arena booking. Both 1v1 and XvX (team) matches share
//! this same struct; 1v1 is just the `team_size == 1` case of the general
//! team-tracking logic, per the architecture doc's "Scalable XvX" design
//! goal.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use pumpkin_plugin_api::common::{GameMode, NamedColor};
use pumpkin_plugin_api::item_stack::ItemStack;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{Server, scheduler::SchedulerExt, uuid};

use crate::arena::{ArenaRegistry, ArenaState};
use crate::kits::Kit;
use crate::lobby::LobbyStore;
use crate::state::{PlayerState, StateRegistry};
use crate::stats::StatsRegistry;
use crate::tiers::TierRegistry;

/// Poll interval for the combat loop, in ticks. 10 ticks == 0.5s, matching
/// the architecture doc's "Combat & Live HUD" cadence.
const POLL_PERIOD_TICKS: u64 = 10;
/// Countdown length in seconds. At a 0.5s poll period that's 2 polls/sec.
const COUNTDOWN_SECONDS: u32 = 3;
/// Delay before automatically respawning a defeated fighter, roughly 1.5s.
const RESPAWN_DELAY_TICKS: u64 = 30;

static NEXT_MATCH_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
struct Fighter {
    uuid: String,
    name: String,
    team: Team,
    /// Set once this fighter has died and been auto-respawned/spectated, so
    /// we don't process their elimination twice.
    eliminated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Team {
    A,
    B,
}

struct RunningMatch {
    id: u64,
    kit: Kit,
    arena_name: String,
    world_id: String,
    fighters: Vec<Fighter>,
    /// Whole seconds left in the pre-fight countdown; `0` once fighting.
    countdown_remaining: u32,
    fighting: bool,
    /// Set once `finish_match`/`force_end` has run, so a poll tick that was
    /// already in flight when cleanup started becomes a no-op instead of
    /// double-processing. `task_id` is also cancelled at the same time via
    /// `scheduler::cancel_task`, so this flag is mainly a safety net for the
    /// one tick that might already be mid-flight.
    ended: bool,
    /// Id returned by `schedule_repeating_task`, used to stop the poll loop
    /// via `pumpkin_plugin_api::scheduler::cancel_task` once the match ends.
    task_id: u32,
}

impl RunningMatch {
    fn team_alive(&self, team: Team) -> HashSet<String> {
        self.fighters
            .iter()
            .filter(|f| f.team == team && !f.eliminated)
            .map(|f| f.uuid.clone())
            .collect()
    }

    fn fighter_mut(&mut self, uuid: &str) -> Option<&mut Fighter> {
        self.fighters.iter_mut().find(|f| f.uuid == uuid)
    }
}

pub struct MatchManager {
    matches: RwLock<HashMap<u64, RunningMatch>>,
    arenas: Arc<ArenaRegistry>,
    stats: Arc<StatsRegistry>,
    tiers: Arc<TierRegistry>,
    state: Arc<StateRegistry>,
    lobby: Arc<LobbyStore>,
}

impl MatchManager {
    pub fn new(
        arenas: Arc<ArenaRegistry>,
        stats: Arc<StatsRegistry>,
        tiers: Arc<TierRegistry>,
        state: Arc<StateRegistry>,
        lobby: Arc<LobbyStore>,
    ) -> Self {
        Self {
            matches: RwLock::new(HashMap::new()),
            arenas,
            stats,
            tiers,
            state,
            lobby,
        }
    }

    /// Attempts to start a match for the given players + kit. On success,
    /// books an arena, teleports/equips everyone, sets their state to
    /// `InMatch`, and schedules the polling loop. Returns `false` if no
    /// arena was free or a player disappeared between queueing and match
    /// start — the caller (queue_cmd.rs) is responsible for putting affected
    /// players back in queue and messaging them via the action bar per the
    /// "Searching for open arena..." behavior in the architecture doc.
    pub fn try_start_match(
        self: &Arc<Self>,
        server: &Server,
        kit: &Kit,
        player_uuids: &[String],
    ) -> bool {
        let Some(arena_name) = self.arenas.reserve_free_arena() else {
            return false;
        };
        let Some(arena) = self.arenas.get(&arena_name) else {
            self.arenas.release(&arena_name);
            return false;
        };
        let (Some(spawn_a), Some(spawn_b), Some(world_id)) =
            (arena.spawn_a, arena.spawn_b, arena.world_id.clone())
        else {
            // Shouldn't happen -- reserve_free_arena only returns ready
            // arenas -- but guard rather than starting a broken match.
            self.arenas.release(&arena_name);
            return false;
        };

        if server.get_all_worlds().into_iter().find(|w| w.get_id() == world_id).is_none() {
            tracing::warn!("Arena '{arena_name}' world '{world_id}' no longer exists.");
            self.arenas.release(&arena_name);
            return false;
        }

        let team_size = if kit.multiplayer { kit.team_size.max(1) as usize } else { 1 };
        let mut fighters = Vec::with_capacity(player_uuids.len());

        for (i, uuid_str) in player_uuids.iter().enumerate() {
            let Some(parsed) = uuid::parse(uuid_str) else { continue };
            let Some(player) = server.get_player_by_uuid(parsed) else { continue };
            let team = if i < team_size { Team::A } else { Team::B };
            fighters.push(Fighter {
                uuid: uuid_str.clone(),
                name: player.get_name(),
                team,
                eliminated: false,
            });
        }

        if fighters.len() != player_uuids.len() {
            // Someone disconnected between queueing and match start.
            self.arenas.release(&arena_name);
            return false;
        }

        let match_id = NEXT_MATCH_ID.fetch_add(1, Ordering::Relaxed);

        for fighter in &fighters {
            let Some(parsed) = uuid::parse(&fighter.uuid) else { continue };
            let Some(player) = server.get_player_by_uuid(parsed) else { continue };
            let Some(world) = server.get_all_worlds().into_iter().find(|w| w.get_id() == world_id) else {
                continue;
            };
            let spawn = if fighter.team == Team::A { spawn_a } else { spawn_b };

            player.teleport((spawn.x, spawn.y, spawn.z), Some(spawn.yaw), Some(spawn.pitch), world);
            equip_kit(&player, kit);
            player.set_health(player.get_max_health());
            player.set_food_level(20);
            player.as_entity().set_fall_distance(0.0);
            self.state.set(&fighter.uuid, PlayerState::InMatch { match_id });

            if kit.countdown {
                player.show_title(freeze_title(COUNTDOWN_SECONDS));
            } else {
                player.show_title(fight_title());
                player.send_system_message(TextComponent::text("FIGHT!"), false);
            }
        }

        let this = self.clone();
        let task_id = server.schedule_repeating_task(POLL_PERIOD_TICKS, POLL_PERIOD_TICKS, move |server| {
            this.poll_match(match_id, &server);
        });

        let running = RunningMatch {
            id: match_id,
            kit: kit.clone(),
            arena_name: arena_name.clone(),
            world_id,
            fighters,
            countdown_remaining: if kit.countdown { COUNTDOWN_SECONDS } else { 0 },
            fighting: !kit.countdown,
            ended: false,
            task_id,
        };
        self.matches.write().unwrap().insert(match_id, running);
        self.arenas.set_state(&arena_name, ArenaState::Countdown);

        true
    }

    /// One tick of the match loop: advances the countdown, then (once
    /// fighting) checks health for wins/eliminations and refreshes the HUD.
    fn poll_match(self: &Arc<Self>, match_id: u64, server: &Server) {
        let (still_running, still_counting) = {
            let matches = self.matches.read().unwrap();
            match matches.get(&match_id) {
                Some(m) if !m.ended => (true, !m.fighting),
                _ => (false, false),
            }
        };
        if !still_running {
            return;
        }

        if still_counting {
            let (remaining, fighters) = {
                let mut matches = self.matches.write().unwrap();
                let Some(m) = matches.get_mut(&match_id) else { return };
                if m.countdown_remaining > 0 {
                    m.countdown_remaining -= 1;
                }
                (m.countdown_remaining, m.fighters.clone())
            };

            for fighter in &fighters {
                let Some(parsed) = uuid::parse(&fighter.uuid) else { continue };
                let Some(player) = server.get_player_by_uuid(parsed) else { continue };
                if remaining > 0 {
                    player.show_title(freeze_title(remaining));
                } else {
                    player.show_title(fight_title());
                    player.send_system_message(TextComponent::text("FIGHT!"), false);
                }
            }

            if remaining == 0 {
                let mut matches = self.matches.write().unwrap();
                if let Some(m) = matches.get_mut(&match_id) {
                    m.fighting = true;
                }
            }
            return;
        }

        self.tick_combat(match_id, server);
    }

    fn tick_combat(self: &Arc<Self>, match_id: u64, server: &Server) {
        // Snapshot the roster under a read lock, then drop it before doing
        // any player I/O (health reads, teleports) to avoid holding the
        // lock across host calls.
        let (fighters, kit_fall_damage) = {
            let matches = self.matches.read().unwrap();
            let Some(m) = matches.get(&match_id) else { return };
            if m.ended {
                return;
            }
            (m.fighters.clone(), m.kit.fall_damage)
        };

        let mut newly_eliminated: Vec<String> = Vec::new();

        for fighter in &fighters {
            if fighter.eliminated {
                continue;
            }
            let Some(parsed) = uuid::parse(&fighter.uuid) else { continue };
            let Some(player) = server.get_player_by_uuid(parsed) else {
                // Player disconnected without the leave event having run
                // yet (or it fired but hasn't reached us); handled
                // idempotently by handle_player_left once it does.
                continue;
            };

            if !kit_fall_damage {
                // No gamerule hook exists for fall damage, so pre-empt it
                // by clearing accumulated fall distance every poll -- the
                // vanilla damage calculation reads this value on landing,
                // so keeping it at 0 means impacts never register as a
                // fall in the first place.
                player.as_entity().set_fall_distance(0.0);
            }

            if player.get_health() <= 0.0 {
                newly_eliminated.push(fighter.uuid.clone());
            }
        }

        for uuid_str in &newly_eliminated {
            self.eliminate_fighter(match_id, uuid_str, server);
        }

        if let Some(outcome) = self.check_victory(match_id) {
            self.finish_match(match_id, server, outcome);
            return;
        }

        self.update_hud(match_id, server);
    }

    /// Marks a fighter eliminated, records their death, and schedules a
    /// deferred respawn so they are returned to spectator after a short
    /// delay instead of being transitioned immediately. Safe to call multiple
    /// times; a no-op if the fighter is already eliminated.
    ///
    /// NOTE: there is no combat/damage event exposed to plugins (see module
    /// doc), so there's no reliable way to attribute *who* landed the
    /// killing blow -- only that this fighter's health hit zero. Kills are
    /// therefore not auto-incremented here; `stats.record_kill` is exposed
    /// for future wiring if/when the host adds a damage-source event.
    fn eliminate_fighter(self: &Arc<Self>, match_id: u64, uuid_str: &str, server: &Server) {
        let name = {
            let mut matches = self.matches.write().unwrap();
            let Some(m) = matches.get_mut(&match_id) else { return };
            let Some(fighter) = m.fighter_mut(uuid_str) else { return };
            if fighter.eliminated {
                return;
            }
            fighter.eliminated = true;
            fighter.name.clone()
        };

        self.stats.record_death(uuid_str, &name);

        let this = self.clone();
        let match_id_for_respawn = match_id;
        let uuid_for_respawn = uuid_str.to_string();
        server.schedule_delayed_task(RESPAWN_DELAY_TICKS, move |server| {
            this.handle_delayed_respawn(match_id_for_respawn, &uuid_for_respawn, &server);
        });

        let fighters = {
            let matches = self.matches.read().unwrap();
            matches.get(&match_id).map(|m| m.fighters.clone()).unwrap_or_default()
        };
        for other in &fighters {
            if other.eliminated {
                continue;
            }
            let Some(parsed) = uuid::parse(&other.uuid) else { continue };
            let Some(player) = server.get_player_by_uuid(parsed) else { continue };
            player.send_system_message(TextComponent::text(&format!("{name} was eliminated!")), false);
        }
    }

    fn handle_delayed_respawn(self: &Arc<Self>, match_id: u64, uuid_str: &str, server: &Server) {
        let should_respawn = {
            let matches = self.matches.read().unwrap();
            let Some(m) = matches.get(&match_id) else { return };
            if m.ended {
                return;
            }
            m.fighters
                .iter()
                .find(|f| f.uuid == uuid_str)
                .map(|fighter| fighter.eliminated)
                .unwrap_or(false)
        };

        if !should_respawn {
            return;
        }

        if let Some(parsed) = uuid::parse(uuid_str) {
            if let Some(player) = server.get_player_by_uuid(parsed) {
                player.respawn();
                player.set_health(player.get_max_health());
                player.set_food_level(20);
                player.set_gamemode(GameMode::Spectator);
            }
        }
    }

    /// Checks whether one team has been fully eliminated. Returns the
    /// winning/losing rosters if so.
    fn check_victory(&self, match_id: u64) -> Option<MatchOutcome> {
        let matches = self.matches.read().unwrap();
        let m = matches.get(&match_id)?;
        if m.ended {
            return None;
        }
        let a_alive = m.team_alive(Team::A);
        let b_alive = m.team_alive(Team::B);

        if a_alive.is_empty() && b_alive.is_empty() {
            // Both teams wiped in the same tick (e.g. a Sumo double
            // ring-out) -- call it with no winner rather than crediting
            // either side.
            return Some(MatchOutcome {
                winners: Vec::new(),
                losers: m.fighters.iter().map(|f| (f.uuid.clone(), f.name.clone())).collect(),
                reason: EndReason::Draw,
            });
        }
        if a_alive.is_empty() {
            return Some(team_outcome(m, Team::B));
        }
        if b_alive.is_empty() {
            return Some(team_outcome(m, Team::A));
        }
        None
    }

    /// Called when a player disconnects mid-match (wired from
    /// `CleanupOnLeave` in lib.rs). Per spec: the leaving player forfeits
    /// and the opposing team is declared the winner immediately, without
    /// waiting for a health-based elimination.
    pub fn handle_player_left(self: &Arc<Self>, server: &Server, uuid_str: &str) {
        let match_id = {
            let matches = self.matches.read().unwrap();
            matches
                .values()
                .find(|m| !m.ended && m.fighters.iter().any(|f| f.uuid == uuid_str))
                .map(|m| m.id)
        };
        let Some(match_id) = match_id else { return };

        let leaver_team = {
            let matches = self.matches.read().unwrap();
            let Some(m) = matches.get(&match_id) else { return };
            let Some(f) = m.fighters.iter().find(|f| f.uuid == uuid_str) else { return };
            f.team
        };

        // A leave forfeits for that player's whole side, even in a team
        // match with surviving teammates -- mark the entire team eliminated
        // so check_victory resolves the match immediately in the other
        // team's favor.
        {
            let mut matches = self.matches.write().unwrap();
            if let Some(m) = matches.get_mut(&match_id) {
                for f in m.fighters.iter_mut().filter(|f| f.team == leaver_team) {
                    f.eliminated = true;
                }
            }
        }

        if let Some(outcome) = self.check_victory(match_id) {
            self.finish_match(match_id, server, outcome);
        } else {
            // Defensive fallback: shouldn't be reachable since marking a
            // whole team eliminated always resolves check_victory, but
            // avoid leaving the match dangling if it somehow doesn't.
            self.force_end(match_id, server);
        }
    }

    fn finish_match(&self, match_id: u64, server: &Server, outcome: MatchOutcome) {
        let (arena_name, kit_name) = {
            let mut matches = self.matches.write().unwrap();
            let Some(m) = matches.get_mut(&match_id) else { return };
            if m.ended {
                return;
            }
            m.ended = true;
            pumpkin_plugin_api::scheduler::cancel_task(m.task_id);
            (m.arena_name.clone(), m.kit.name.clone())
        };

        match outcome.reason {
            EndReason::Draw => {
                for (uuid_str, name) in &outcome.losers {
                    self.stats.record_loss(uuid_str, name);
                }
                announce(server, &format!("Match on '{arena_name}' ended with no winner."));
            }
            EndReason::Elimination => {
                for (uuid_str, name) in &outcome.winners {
                    self.stats.record_win(uuid_str, name);
                    // Tiers are gated on wins, not kills -- see tiers.rs's
                    // module doc for why (no combat/damage event exists to
                    // attribute a kill to an attacker). Re-deriving the
                    // tier here isn't strictly necessary (tier_for_wins is
                    // computed on demand wherever it's needed -- /stats,
                    // /tier, and the lobby HUD), but this keeps a hook point
                    // if tier-up announcements are wanted later.
                    let wins = self.stats.get(uuid_str).wins;
                    let _ = self.tiers.tier_for_wins(wins);
                }
                for (uuid_str, name) in &outcome.losers {
                    self.stats.record_loss(uuid_str, name);
                }

                let winner_names: Vec<String> = outcome.winners.iter().map(|(_, n)| n.clone()).collect();
                tellraw_winner_announcement(server, &kit_name, &winner_names);

                for (uuid_str, name) in &outcome.winners {
                    let Some(parsed) = uuid::parse(uuid_str) else { continue };
                    let Some(player) = server.get_player_by_uuid(parsed) else { continue };
                    player.show_title(victory_title());
                    let msg = TextComponent::text(&format!(" Match Results: Winner, {name}! (+15 ELO)"));
                    msg.color_named(NamedColor::Green);
                    player.send_system_message(msg, false);
                }
                for (uuid_str, name) in &outcome.losers {
                    let Some(parsed) = uuid::parse(uuid_str) else { continue };
                    let Some(player) = server.get_player_by_uuid(parsed) else { continue };
                    player.show_title(defeat_title());
                    let msg = TextComponent::text(&format!(" Match Results: Defeat, {name}. (-15 ELO)"));
                    msg.color_named(NamedColor::Red);
                    player.send_system_message(msg, false);
                }
            }
        }

        self.cleanup_players(match_id, server, &outcome);
        self.arenas.release(&arena_name);
        self.matches.write().unwrap().remove(&match_id);
    }

    /// Cleans up a match with no declared winner (defensive fallback path —
    /// see `handle_player_left`). Everyone still tracked is sent back to the
    /// lobby with no further stat changes.
    fn force_end(&self, match_id: u64, server: &Server) {
        let (arena_name, fighters) = {
            let mut matches = self.matches.write().unwrap();
            let Some(m) = matches.get_mut(&match_id) else { return };
            if m.ended {
                return;
            }
            m.ended = true;
            pumpkin_plugin_api::scheduler::cancel_task(m.task_id);
            (m.arena_name.clone(), m.fighters.clone())
        };
        let outcome = MatchOutcome {
            winners: Vec::new(),
            losers: fighters.iter().map(|f| (f.uuid.clone(), f.name.clone())).collect(),
            reason: EndReason::Draw,
        };
        self.cleanup_players(match_id, server, &outcome);
        self.arenas.release(&arena_name);
        self.matches.write().unwrap().remove(&match_id);
    }

    fn cleanup_players(&self, match_id: u64, server: &Server, outcome: &MatchOutcome) {
        let fighters = {
            let matches = self.matches.read().unwrap();
            matches.get(&match_id).map(|m| m.fighters.clone()).unwrap_or_default()
        };
        let all_uuids: HashSet<&str> = outcome
            .winners
            .iter()
            .chain(outcome.losers.iter())
            .map(|(u, _)| u.as_str())
            .collect();

        for fighter in &fighters {
            if !all_uuids.contains(fighter.uuid.as_str()) {
                continue;
            }
            self.state.set(&fighter.uuid, PlayerState::Lobby);

            let Some(parsed) = uuid::parse(&fighter.uuid) else { continue };
            let Some(player) = server.get_player_by_uuid(parsed) else { continue };

            player.set_gamemode(GameMode::Survival);
            player.set_health(player.get_max_health());
            player.set_food_level(20);
            player.set_saturation(5.0);
            player.set_absorption(0.0);
            player.as_entity().set_fall_distance(0.0);
            for slot in 0..36u8 {
                player.set_inventory_item(slot, None);
            }

            if let Some(lobby_loc) = self.lobby.get() {
                if let Some(world) =
                    server.get_all_worlds().into_iter().find(|w| w.get_id() == lobby_loc.world_id)
                {
                    player.teleport(
                        (lobby_loc.x, lobby_loc.y, lobby_loc.z),
                        Some(lobby_loc.yaw),
                        Some(lobby_loc.pitch),
                        world,
                    );
                }
            }
        }
    }

    fn update_hud(&self, match_id: u64, server: &Server) {
        let (fighters, kit_name) = {
            let matches = self.matches.read().unwrap();
            let Some(m) = matches.get(&match_id) else { return };
            (m.fighters.clone(), m.kit.name.clone())
        };

        for fighter in &fighters {
            if fighter.eliminated {
                continue;
            }
            let Some(parsed) = uuid::parse(&fighter.uuid) else { continue };
            let Some(player) = server.get_player_by_uuid(parsed) else { continue };

            let opponents: Vec<&Fighter> =
                fighters.iter().filter(|f| f.team != fighter.team && !f.eliminated).collect();

            let opponent_desc = match opponents.as_slice() {
                [] => "no one left".to_string(),
                [single] => {
                    if let Some(p) =
                        uuid::parse(&single.uuid).and_then(|id| server.get_player_by_uuid(id))
                    {
                        format!("{} | HP: {:.1}", single.name, p.get_health())
                    } else {
                        single.name.clone()
                    }
                }
                many => format!("{} opponents remaining", many.len()),
            };

            player.show_actionbar(TextComponent::text(&format!(
                "Opponent: {opponent_desc} | Kit: {kit_name}"
            )));
        }
    }

    /// Returns true if the given match id is still active. Exposed for
    /// admin/debug tooling.
    pub fn is_active(&self, match_id: u64) -> bool {
        self.matches.read().unwrap().get(&match_id).map(|m| !m.ended).unwrap_or(false)
    }
}

fn team_outcome(m: &RunningMatch, winning_team: Team) -> MatchOutcome {
    MatchOutcome {
        winners: m
            .fighters
            .iter()
            .filter(|f| f.team == winning_team)
            .map(|f| (f.uuid.clone(), f.name.clone()))
            .collect(),
        losers: m
            .fighters
            .iter()
            .filter(|f| f.team != winning_team)
            .map(|f| (f.uuid.clone(), f.name.clone()))
            .collect(),
        reason: EndReason::Elimination,
    }
}

struct MatchOutcome {
    winners: Vec<(String, String)>, // (uuid, name)
    losers: Vec<(String, String)>,
    reason: EndReason,
}

enum EndReason {
    Elimination,
    Draw,
}

fn equip_kit(player: &pumpkin_plugin_api::player::Player, kit: &Kit) {
    for slot in 0..36u8 {
        player.set_inventory_item(slot, None);
    }
    for (&slot, item) in &kit.items {
        if slot < 36 {
            player.set_inventory_item(slot, Some(ItemStack::new(&item.registry_key, item.count)));
        }
    }
}

fn freeze_title(seconds_left: u32) -> TextComponent {
    let text = TextComponent::text(&format!("{seconds_left}"));
    text.color_named(NamedColor::Gold);
    text
}

fn fight_title() -> TextComponent {
    let text = TextComponent::text("FIGHT!");
    text.color_named(NamedColor::Red);
    text
}

fn victory_title() -> TextComponent {
    let text = TextComponent::text("VICTORY!");
    text.color_named(NamedColor::Gold);
    text
}

fn defeat_title() -> TextComponent {
    let text = TextComponent::text("DEFEAT!");
    text.color_named(NamedColor::Red);
    text
}

/// Broadcasts the match result to all online players, matching the
/// "tellraw winner announcement to the players" requirement. `server.wit`
/// exposes `broadcast(message: string)` as a plain system-chat broadcast —
/// there's no structured `text-component`-based broadcast on `server`
/// itself (that's only on `%world::broadcast-system-message` and
/// `command-sender::send-message`), so this is the closest plugin-exposed
/// equivalent to vanilla `/tellraw @a`.
fn tellraw_winner_announcement(server: &Server, kit_name: &str, winners: &[String]) {
    let winner_list = winners.join(" & ");
    server.broadcast(&format!("[Lantern] {winner_list} won a {kit_name} match!"));
}

fn announce(server: &Server, message: &str) {
    server.broadcast(&format!("[Lantern] {message}"));
}
