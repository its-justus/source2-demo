use crate::entity::field::*;
use crate::entity::*;
use crate::event::*;
use crate::string_table::*;
use crate::HashMap;
use source2_demo_protobufs::CDemoFileInfo;
use std::fmt::Display;
use std::rc::Rc;

/// Current replay state.
pub struct Context {
    pub(crate) classes: Classes,
    pub(crate) entities: Entities,
    pub(crate) string_tables: StringTables,
    pub(crate) game_events: GameEventList,

    pub(crate) tick: u32,
    pub(crate) previous_tick: u32,
    pub(crate) net_tick: u32,

    pub(crate) game_build: u32,
    pub(crate) replay_info: CDemoFileInfo,

    pub(crate) baselines: BaselineContainer,
    pub(crate) serializers: HashMap<Box<str>, Rc<Serializer>>,
    pub(crate) last_full_packet_tick: u32,
    pub(crate) packet_stats: PacketStats,
}

impl Default for Context {
    fn default() -> Self {
        Context {
            classes: Classes::default(),
            entities: Entities::default(),
            string_tables: StringTables::default(),
            game_events: Default::default(),
            tick: u32::MAX,
            previous_tick: u32::MAX,
            net_tick: u32::MAX,
            game_build: 0,
            replay_info: CDemoFileInfo::default(),
            baselines: BaselineContainer::default(),
            serializers: HashMap::default(),
            last_full_packet_tick: u32::MAX,
            packet_stats: PacketStats::default(),
        }
    }
}

impl Context {
    pub fn new(replay_info: CDemoFileInfo) -> Self {
        Context {
            replay_info,
            ..Default::default()
        }
    }
}

impl Context {
    pub fn classes(&self) -> &Classes {
        &self.classes
    }

    pub fn entities(&self) -> &Entities {
        &self.entities
    }

    pub fn string_tables(&self) -> &StringTables {
        &self.string_tables
    }

    pub fn game_events(&self) -> &GameEventList {
        &self.game_events
    }

    pub fn tick(&self) -> u32 {
        self.tick
    }

    pub fn net_tick(&self) -> u32 {
        self.net_tick
    }

    pub fn game_build(&self) -> u32 {
        self.game_build
    }

    pub fn replay_info(&self) -> &CDemoFileInfo {
        &self.replay_info
    }

    pub fn packet_stats(&self) -> &PacketStats {
        &self.packet_stats
    }
}

pub struct PacketStats {
    packet_count: u32,
    message_count: u32,
    message_type_counts: HashMap<u32, u32>,
}

impl PacketStats {
    pub fn packets(&self) -> u32 {
        self.packet_count
    }

    pub fn add_packet(&mut self) {
        self.packet_count += 1;
    }

    pub fn messages(&self) -> u32 {
        self.message_count
    }

    pub fn add_message(&mut self, m_type: u32) {
        self.message_count += 1;
        self.message_type_counts
            .entry(m_type)
            .and_modify(|e| *e += 1)
            .or_insert(1);
    }
}

impl Default for PacketStats {
    fn default() -> Self {
        Self {
            packet_count: 0,
            message_count: 0,
            message_type_counts: Default::default(),
        }
    }
}

impl Display for PacketStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let m_types: String = self
            .message_type_counts
            .iter()
            .map(|e| format!("\n{}: {}", e.0, e.1))
            .collect();
        write!(
            f,
            "packets: {}, messages: {}{}",
            self.packet_count, self.message_count, m_types
        )
    }
}
