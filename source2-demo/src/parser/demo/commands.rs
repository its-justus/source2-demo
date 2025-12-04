use crate::entity::field::*;
use crate::entity::Class;
use crate::error::ParserError;
use crate::parser::demo::DemoMessages;
use crate::proto::*;
use crate::reader::*;
use crate::HashMap;
use crate::PacketStats;
use crate::{Parser, StringTableRow};
use std::fmt::Display;
use std::fs::File;
use std::io::Write;
use std::rc::Rc;

pub trait DemoCommands {
    fn dem_send_tables(&mut self, send_tables: CDemoSendTables) -> Result<(), ParserError>;

    fn dem_class_info(&mut self, class_info: CDemoClassInfo) -> Result<(), ParserError>;

    fn dem_packet(&mut self, demo_packet: CDemoPacket) -> Result<(), ParserError>;

    fn dem_full_packet(&mut self, full_packet: CDemoFullPacket) -> Result<(), ParserError>;

    fn dem_string_tables(&mut self, string_tables: CDemoStringTables) -> Result<(), ParserError>;

    fn dem_stop(&mut self) -> Result<(), ParserError> {
        Ok(())
    }
}

impl DemoCommands for Parser<'_> {
    fn dem_send_tables(&mut self, send_tables: CDemoSendTables) -> Result<(), ParserError> {
        let serializers = &mut self.context.serializers;

        let mut reader = Reader::new(send_tables.data());
        let amount = reader.read_var_u32();
        let buf = reader.read_bytes(amount);

        let fs = CSvcMsgFlattenedSerializer::decode(buf.as_slice())?;

        let resolve = |p: Option<i32>| -> &str {
            if let Some(i) = p {
                return &fs.symbols[i as usize];
            }
            ""
        };

        let mut fields: Vec<Rc<Field>> = vec![];
        let mut field_types: HashMap<&str, Rc<FieldType>> = HashMap::default();

        for s in fs.serializers.iter() {
            let ser_name = resolve(s.serializer_name_sym);
            let mut serializer = Serializer::default();

            for i in s.fields_index.iter().map(|&x| x as usize) {
                let current_field = &fs.fields[i];
                let field_serializer_name = resolve(current_field.field_serializer_name_sym);

                if i >= fields.len() {
                    let var_type_str = resolve(current_field.var_type_sym);
                    let var_name = resolve(current_field.var_name_sym);

                    let current_field_serializer = serializers.get(field_serializer_name);

                    let field_type = field_types
                        .entry(var_type_str)
                        .or_insert_with(|| Rc::new(FieldType::new(var_type_str)))
                        .clone();

                    let properties = FieldProperties {
                        encoder: match var_name {
                            "m_flSimulationTime" | "m_flAnimTime" => Some(FieldEncoder::SimTime),
                            "m_flRuneTime" => Some(FieldEncoder::RuneTime),
                            _ => FieldEncoder::from_str(resolve(current_field.var_encoder_sym)),
                        },
                        encoder_flags: current_field.encode_flags(),
                        bit_count: current_field.bit_count(),
                        low_value: current_field.low_value(),
                        high_value: current_field.high_value(),
                    };

                    let model = if let Some(ser) = current_field_serializer {
                        if field_type.pointer {
                            FieldModel::Pointer(ser.clone())
                        } else {
                            FieldModel::Vector(ser.clone())
                        }
                    } else if matches!(
                        field_type.base.as_ref(),
                        "CUtlVector" | "CNetworkUtlVectorBase" | "CUtlVectorEmbeddedNetworkVar"
                    ) {
                        FieldModel::ArrayVector(FieldDecoder::from_field(
                            field_type.generic.as_ref().unwrap(),
                            properties,
                        ))
                    } else if field_type.count > 0 && field_type.base.as_ref() != "char" {
                        FieldModel::Array
                    } else {
                        FieldModel::Value
                    };

                    let mut decoder = match model {
                        FieldModel::Value | FieldModel::Array => {
                            FieldDecoder::from_field(&field_type, properties)
                        }
                        FieldModel::Vector(_) | FieldModel::ArrayVector(_) => {
                            FieldDecoder::Unsigned32
                        }
                        FieldModel::Pointer(_) => FieldDecoder::Boolean,
                    };

                    if ser_name == "CCSGameModeRules" || var_name == "m_pGameModeRules" {
                        decoder = FieldDecoder::CCSGameModeRules;
                    }

                    let field = Field {
                        var_name: var_name.into(),
                        field_type,
                        model,
                        decoder,
                    };
                    fields.push(field.into());
                }
                serializer.fields.push(Rc::clone(&fields[i]));
            }
            serializers.insert(ser_name.into(), serializer.into());
        }
        Ok(())
    }

    fn dem_class_info(&mut self, class_info: CDemoClassInfo) -> Result<(), ParserError> {
        for class in class_info.classes {
            let class_id = class.class_id();
            let network_name = class.network_name();
            let serializer = self.context.serializers[network_name].clone();
            let class = Rc::new(Class::new(class_id, network_name.into(), serializer));

            self.context.classes.classes_vec.push(class.clone());
            self.context
                .classes
                .classes_by_name
                .insert(network_name.into(), class);
        }
        Ok(())
    }

    fn dem_packet(&mut self, packet: CDemoPacket) -> Result<(), ParserError> {
        let mut packet_reader = Reader::new(packet.data());
        self.context.packet_stats = PacketStats::default();
        self.context.packet_stats.add_packet();
        let mut messages: Vec<DecodedMessage> = Vec::new();
        while packet_reader.bytes_remaining() != 0 {
            let msg_type = packet_reader.read_ubit_var() as i32;
            let size = packet_reader.read_var_u32();
            let msg_buf = packet_reader.read_bytes(size);

            // start: my stuff
            self.context.packet_stats.add_message(msg_type as u32);
            let m_type = RawMessageType::from(msg_type);

            if m_type != RawMessageType::Unknown {
                messages.push(DecodedMessage::from((m_type, msg_buf.as_slice())));
            }

            // end: my stuff

            #[cfg(feature = "deadlock")]
            if let Ok(msg) = CitadelUserMessageIds::try_from(msg_type) {
                self.on_citadel_user_message(msg, &msg_buf)?;
                continue;
            } else if let Ok(msg) = ECitadelGameEvents::try_from(msg_type) {
                self.on_citadel_game_event(msg, &msg_buf)?;
                continue;
            }

            if let Ok(msg) = SvcMessages::try_from(msg_type) {
                self.on_svc_message(msg, &msg_buf)?;
            } else if let Ok(msg) = EBaseUserMessages::try_from(msg_type) {
                self.on_base_user_message(msg, &msg_buf)?;
            } else if let Ok(msg) = EBaseGameEvents::try_from(msg_type) {
                self.on_base_game_event(msg, &msg_buf)?;
            } else if let Ok(msg) = NetMessages::try_from(msg_type) {
                self.on_net_message(msg, &msg_buf)?;
            }
        }
        println!("\n\n{}", &self.context.packet_stats);
        println!("message count: {}", messages.len());
        dump_packet_to_json(messages);
        Ok(())
    }

    fn dem_full_packet(&mut self, full_packet: CDemoFullPacket) -> Result<(), ParserError> {
        if self.context.last_full_packet_tick == u32::MAX || self.skip_deltas {
            self.dem_string_tables(full_packet.string_table.unwrap())?;
            self.dem_packet(full_packet.packet.unwrap())?;
        }

        self.context.last_full_packet_tick = self.context.tick;

        Ok(())
    }

    fn dem_string_tables(&mut self, msg: CDemoStringTables) -> Result<(), ParserError> {
        for table in msg.tables.iter() {
            let x = self
                .context
                .string_tables
                .get_by_name_mut(table.table_name())?;

            x.items
                .resize_with(table.items.len(), StringTableRow::default);
            for (i, item) in table.items.iter().enumerate() {
                x.items[i].index = i as i32;
                x.items[i].key = item.str().to_string();
                x.items[i].value = Rc::new(item.data().to_vec()).into();
                if table.table_name() == "instancebaseline" {
                    self.context.baselines.add_baseline(
                        item.str().parse().unwrap_or(-1),
                        x.items[i].value.as_ref().unwrap().clone(),
                    );
                }
            }
        }

        Ok(())
    }

    fn dem_stop(&mut self) -> Result<(), ParserError> {
        self.on_stop()?;
        Ok(())
    }
}

#[derive(PartialEq)]
enum RawMessageType {
    Base(EBaseGameEvents),
    Game(CitadelUserMessageIds),
    Net(NetMessages),
    Service(SvcMessages),
    Unknown,
}

impl From<i32> for RawMessageType {
    fn from(value: i32) -> Self {
        match value {
            4 => RawMessageType::Net(NetMessages::NetTick),
            210 => RawMessageType::Base(EBaseGameEvents::GeSosSetSoundEventParams),
            55 => RawMessageType::Service(SvcMessages::SvcPacketEntities),
            76 => RawMessageType::Service(SvcMessages::SvcUserCmds),
            _ => RawMessageType::Unknown,
        }
    }
}

impl Display for RawMessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RawMessageType::Base(ebase_game_events) => match ebase_game_events {
                EBaseGameEvents::GeSosSetSoundEventParams => {
                    write!(f, "EBaseGameEvents::GeSosSetSoundEventParams")
                }
                _ => write!(f, "EBaseGameEvents::UnknownMessage"),
            },
            RawMessageType::Game(ecitadel_game_events) => match ecitadel_game_events {
                _ => write!(f, "ECitadelGameEvents::UnknownMessage"),
            },
            RawMessageType::Net(net_messages) => match net_messages {
                NetMessages::NetTick => write!(f, "NetMessages::NetTick"),
                _ => write!(f, "NetMessages::UnknownMessage"),
            },
            RawMessageType::Unknown => write!(f, "UnknownMessageType"),
            RawMessageType::Service(svc_messages) => match svc_messages {
                SvcMessages::SvcPacketEntities => write!(f, "SvcMessages::SvcPacketEntities"),
                SvcMessages::SvcUserCmds => write!(f, "SvcMessages::SvcUserCmds"),
                _ => write!(f, "SvcMessages::UnknownMessage"),
            },
        }
    }
}

fn dump_packet_to_json(messages: Vec<DecodedMessage>) {
    let json = serde_json::to_string(&messages).unwrap();
    let mut outfile = File::options()
        .write(true)
        .create(true)
        .open(format!("../replays/match-details/packet.json"))
        .expect("unable to create file");

    outfile
        .write(json.as_bytes())
        .expect("unable to write file");
}

#[derive(serde::Serialize)]
enum DecodedMessage {
    NetTick(CNetMsgTick),
    GeSosSetSoundEventParams(CMsgSosSetSoundEventParams),
    SvcPacketEntities(CSvcMsgPacketEntities),
    SvcUserCmds(CSvcMsgUserCommands),
}

impl From<(RawMessageType, &[u8])> for DecodedMessage {
    fn from(value: (RawMessageType, &[u8])) -> Self {
        let msg = value.1;
        match value.0 {
            RawMessageType::Net(net_messages) => match net_messages {
                NetMessages::NetTick => DecodedMessage::NetTick(CNetMsgTick::decode(msg).unwrap()),
                _ => panic!("unknown net message"),
            },
            RawMessageType::Game(citadel_user_messages) => todo!(),
            RawMessageType::Base(ebase_game_events) => match ebase_game_events {
                EBaseGameEvents::GeSosSetSoundEventParams => {
                    DecodedMessage::GeSosSetSoundEventParams(
                        CMsgSosSetSoundEventParams::decode(msg).unwrap(),
                    )
                }
                _ => panic!("unknown base game message"),
            },
            RawMessageType::Unknown => todo!(),
            RawMessageType::Service(svc_messages) => match svc_messages {
                SvcMessages::SvcPacketEntities => {
                    DecodedMessage::SvcPacketEntities(CSvcMsgPacketEntities::decode(msg).unwrap())
                }
                SvcMessages::SvcUserCmds => {
                    DecodedMessage::SvcUserCmds(CSvcMsgUserCommands::decode(msg).unwrap())
                }
                _ => panic!("unknown service message"),
            },
        }
    }
}
