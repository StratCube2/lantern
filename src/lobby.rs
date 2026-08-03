//! Lobby spawn point: `/setlobby` stores the caller's current position
//! (world id + x/y/z/yaw/pitch), persisted to `lobby.toml` in the plugin's
//! data folder. On `PlayerJoinEvent` we mark the player for a deferred lobby
//! teleport, then actually teleport them after their first chat event or,
//! for movement, after a 10-tick delay so the server has finished finishing
//! their join.
//!
//! There is no vanilla "world spawn setter" or "player spawn setter" exposed
//! to plugins (checked against player.wit / world.wit / server.wit) — so we
//! own this ourselves rather than trying to touch a bed/anchor-style spawn.

use std::{collections::HashSet, sync::{Arc, RwLock}};

use pumpkin_plugin_api::events::player::PlayerJoinEvent;
use pumpkin_plugin_api::events::{EventHandler, EventPriority, FromIntoEvent};
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::uuid;
use pumpkin_plugin_api::{
    Context, Server,
    command::{CommandError, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    scheduler::SchedulerExt,
};
use serde::{Deserialize, Serialize};

use crate::player_events::{PlayerChatEvent, PlayerMoveEvent};

const LOBBY_MOVE_TELEPORT_DELAY_TICKS: u64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyLocation {
    pub world_id: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

pub struct LobbyStore {
    inner: RwLock<Option<LobbyLocation>>,
    pending_teleports: RwLock<HashSet<String>>,
    scheduled_teleports: RwLock<HashSet<String>>,
}

impl LobbyStore {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(None),
            pending_teleports: RwLock::new(HashSet::new()),
            scheduled_teleports: RwLock::new(HashSet::new()),
        }
    }

    pub fn get(&self) -> Option<LobbyLocation> {
        self.inner.read().unwrap().clone()
    }

    pub fn set(&self, loc: LobbyLocation) {
        *self.inner.write().unwrap() = Some(loc);
    }

    pub fn mark_pending(&self, uuid: &str) {
        self.pending_teleports.write().unwrap().insert(uuid.to_string());
    }

    pub fn clear_pending(&self, uuid: &str) {
        self.pending_teleports.write().unwrap().remove(uuid);
    }

    fn mark_scheduled(&self, uuid: &str) -> bool {
        self.scheduled_teleports.write().unwrap().insert(uuid.to_string())
    }

    fn clear_scheduled(&self, uuid: &str) {
        self.scheduled_teleports.write().unwrap().remove(uuid);
    }

    pub fn try_teleport_pending(
        &self,
        player: &pumpkin_plugin_api::player::Player,
        server: &Server,
    ) -> bool {
        let uuid_str = uuid::to_string(player.get_id());
        self.try_teleport_pending_by_uuid(&uuid_str, server)
    }

    pub fn schedule_move_teleport(self: &Arc<Self>, player: &pumpkin_plugin_api::player::Player, server: &Server) -> bool {
        let uuid_str = uuid::to_string(player.get_id());
        if !self.pending_teleports.read().unwrap().contains(&uuid_str) {
            return false;
        }

        if !self.mark_scheduled(&uuid_str) {
            return false;
        }

        let this = Arc::clone(self);
        let uuid_for_task = uuid_str.clone();
        server.schedule_delayed_task(LOBBY_MOVE_TELEPORT_DELAY_TICKS, move |server| {
            let _ = this.try_teleport_pending_by_uuid(&uuid_for_task, &server);
        });

        true
    }

    fn try_teleport_pending_by_uuid(&self, uuid_str: &str, server: &Server) -> bool {
        if !self.pending_teleports.read().unwrap().contains(uuid_str) {
            self.clear_scheduled(uuid_str);
            return false;
        }

        let Some(loc) = self.get() else {
            self.clear_scheduled(uuid_str);
            self.clear_pending(uuid_str);
            return false;
        };

        let Some(world) = server
            .get_all_worlds()
            .into_iter()
            .find(|w| w.get_id() == loc.world_id)
        else {
            tracing::warn!("Lobby world '{}' no longer exists.", loc.world_id);
            self.clear_scheduled(uuid_str);
            return false;
        };

        let Some(player_uuid) = uuid::parse(uuid_str) else {
            self.clear_scheduled(uuid_str);
            return false;
        };

        let Some(player) = server.get_player_by_uuid(player_uuid) else {
            self.clear_scheduled(uuid_str);
            return false;
        };

        player.teleport((loc.x, loc.y, loc.z), Some(loc.yaw), Some(loc.pitch), world);
        self.clear_pending(uuid_str);
        self.clear_scheduled(uuid_str);
        player.send_system_message(TextComponent::text("Teleported to the lobby."), false);
        true
    }

    fn file_path(data_folder: &str) -> String {
        format!("{data_folder}/lobby.toml")
    }

    pub fn load(&self, data_folder: &str) {
        let path = Self::file_path(data_folder);
        if let Ok(contents) = std::fs::read_to_string(&path) {
            match toml::from_str::<LobbyLocation>(&contents) {
                Ok(loc) => *self.inner.write().unwrap() = Some(loc),
                Err(e) => {
                    tracing::warn!("Failed to parse lobby.toml: {e}");
                }
            }
        }
    }

    pub fn save(&self, data_folder: &str) {
        let Some(loc) = self.get() else { return };
        let path = Self::file_path(data_folder);
        match toml::to_string_pretty(&loc) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&path, text) {
                    tracing::warn!("Failed to write lobby.toml: {e}");
                }
            }
            Err(e) => tracing::warn!("Failed to serialize lobby.toml: {e}"),
        }
    }
}

impl Default for LobbyStore {
    fn default() -> Self {
        Self::new()
    }
}

/// `/setlobby` — saves the caller's current position as the lobby spawn.
pub struct SetLobbyExecutor {
    pub store: std::sync::Arc<LobbyStore>,
    pub data_folder: String,
}

impl CommandHandler for SetLobbyExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            sender.send_message(TextComponent::text("Only players can set the lobby."));
            return Ok(0);
        };

        let (x, y, z) = player.get_position();
        let yaw = player.get_yaw();
        let pitch = player.get_pitch();
        let world_id = player.get_world().get_id();

        self.store.set(LobbyLocation {
            world_id,
            x,
            y,
            z,
            yaw,
            pitch,
        });
        self.store.save(&self.data_folder);

        sender.send_message(TextComponent::text("Lobby spawn set to your current position."));
        Ok(1)
    }
}

/// Marks newly-joined players for a deferred lobby teleport, if one exists.
pub struct TeleportToLobbyOnJoin {
    pub store: std::sync::Arc<LobbyStore>,
}

impl EventHandler<PlayerJoinEvent> for TeleportToLobbyOnJoin {
    fn handle(
        &self,
        _server: Server,
        data: <PlayerJoinEvent as FromIntoEvent>::Data,
    ) -> <PlayerJoinEvent as FromIntoEvent>::Data {
        if self.store.get().is_some() {
            let uuid_str = uuid::to_string(data.player.get_id());
            self.store.mark_pending(&uuid_str);
        }
        data
    }
}

pub struct TeleportPendingOnAction {
    pub store: std::sync::Arc<LobbyStore>,
}

impl EventHandler<PlayerChatEvent> for TeleportPendingOnAction {
    fn handle(
        &self,
        server: Server,
        data: <PlayerChatEvent as FromIntoEvent>::Data,
    ) -> <PlayerChatEvent as FromIntoEvent>::Data {
        let _ = self.store.try_teleport_pending(&data.player, &server);
        data
    }
}

impl EventHandler<PlayerMoveEvent> for TeleportPendingOnAction {
    fn handle(
        &self,
        server: Server,
        data: <PlayerMoveEvent as FromIntoEvent>::Data,
    ) -> <PlayerMoveEvent as FromIntoEvent>::Data {
        let _ = self.store.schedule_move_teleport(&data.player, &server);
        data
    }
}

pub fn register(
    context: &Context,
    store: std::sync::Arc<LobbyStore>,
    data_folder: String,
) -> pumpkin_plugin_api::Result<()> {
    store.load(&data_folder);

    context.register_event_handler::<PlayerJoinEvent, _>(
        TeleportToLobbyOnJoin {
            store: store.clone(),
        },
        EventPriority::Normal,
        false,
    )?;

    context.register_event_handler::<PlayerChatEvent, _>(
        TeleportPendingOnAction {
            store: store.clone(),
        },
        EventPriority::Normal,
        false,
    )?;

    context.register_event_handler::<PlayerMoveEvent, _>(
        TeleportPendingOnAction { store },
        EventPriority::Normal,
        false,
    )?;

    Ok(())
}
