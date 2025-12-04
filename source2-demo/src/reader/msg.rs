use crate::error::ParserError;
#[cfg(feature = "deadlock")]
use crate::proto::{
    CCitadelUserMsgPostMatchDetails, CDemoPacket, CMsgMatchMetaDataContents, CitadelUserMessageIds,
};
use crate::proto::{CDemoFileInfo, EDemoCommands, Message};
use crate::reader::bits::BitsReader;
use crate::reader::Reader;

pub(crate) struct OuterMessage {
    pub(crate) msg_type: EDemoCommands,
    pub(crate) tick: u32,
    pub(crate) buf: Vec<u8>,
}

pub(crate) trait MessageReader {
    fn read_next_message(&mut self) -> Result<Option<OuterMessage>, ParserError>;

    fn read_replay_info(&mut self) -> Result<CDemoFileInfo, ParserError>;

    #[cfg(feature = "deadlock")]
    fn read_deadlock_match_details(&mut self) -> Result<CMsgMatchMetaDataContents, ParserError>;
}

impl MessageReader for Reader<'_> {
    #[inline]
    fn read_next_message(&mut self) -> Result<Option<OuterMessage>, ParserError> {
        // check that we still have bytes remaining in our buffer
        if self.bytes_remaining() == 0 {
            return Ok(None);
        }

        // read the command type
        let cmd = self.read_var_u32() as i32;
        // read server tick number
        let tick = self.read_var_u32();
        // read message size in bytes
        let size = self.read_var_u32();

        // convert cmd number to command enum value, masking compression flag bit
        let msg_type = EDemoCommands::try_from(cmd & !(EDemoCommands::DemIsCompressed as i32))?;
        // check if demo is compressed by checking compression flag
        let msg_compressed = cmd & EDemoCommands::DemIsCompressed as i32 != 0;

        // set buffer
        let buf = if msg_compressed {
            // decompress if compressed
            let buf = self.read_bytes(size);
            let mut decoder = snap::raw::Decoder::new();
            decoder.decompress_vec(&buf)?
        } else {
            // otherwise just read
            self.read_bytes(size)
        };

        // return message
        Ok(Some(OuterMessage {
            msg_type,
            tick,
            buf,
        }))
    }

    fn read_replay_info(&mut self) -> Result<CDemoFileInfo, ParserError> {
        let offset = u32::from_le_bytes(self.buf[8..12].try_into().unwrap()) as usize;

        if self.buf.len() < offset {
            return Err(ParserError::ReplayEncodingError);
        }

        let mut reader = Reader::new(&self.buf[offset..]);
        Ok(CDemoFileInfo::decode(
            reader.read_next_message()?.unwrap().buf.as_slice(),
        )?)
    }

    #[cfg(feature = "deadlock")]
    fn read_deadlock_match_details(&mut self) -> Result<CMsgMatchMetaDataContents, ParserError> {
        // create a second temporary reader for some reason
        let mut temp_reader = Reader::new(self.buf);
        // move past the 16 byte preamble
        temp_reader.reset_to(16);

        let mut message_count = 0;
        // loop while we have some message
        while let Some(message) = temp_reader.read_next_message()? {
            message_count += 1;
            // skip packets that aren't DemPackets ()
            if message.msg_type != EDemoCommands::DemPacket {
                println!(
                    "message: {}\t | tick: {}\t | type: {:?}",
                    message_count, message.tick, message.msg_type
                );
                continue;
            }
            if message_count == 18 {
                println!("missing_message:");
                println!(
                    "message: {}\t | tick: {}\t | type: {:?}",
                    message_count, message.tick, message.msg_type
                );
            }

            // decode the message as raw bytes
            let packet = CDemoPacket::decode(message.buf.as_slice())?;

            // make a new reader for the packet data
            let mut packet_reader = Reader::new(packet.data());

            // while we have bytes remaining in the packet
            while packet_reader.bytes_remaining() != 0 {
                // read message type
                let msg_type = packet_reader.read_ubit_var() as i32;
                // read message size
                let size = packet_reader.read_var_u32();
                // get buffer of packet data
                let packet_buf = packet_reader.read_bytes(size);

                // check that the message is the raw match details type
                if msg_type == CitadelUserMessageIds::KEUserMsgPostMatchDetails as i32 {
                    // decode and return match details
                    return Ok(CMsgMatchMetaDataContents::decode(
                        CCitadelUserMsgPostMatchDetails::decode(packet_buf.as_slice())?
                            .match_details(),
                    )?);
                }
            }
        }

        // we didn't find match details packet
        Err(ParserError::MatchDetailsNotFound)
    }
}
