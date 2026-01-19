
/* 
    honestly, i've just stolen this from : 
    https://github.com/webrtc-rs/rtcp/blob/main/src/source_description/mod.rs
*/ 

use bytes::{self, Buf, BufMut, Bytes, BytesMut};

pub struct RTPHeader {
    pub version: u8,
    pub padding: bool,
    pub extension: bool,
    pub marker: bool,
    pub payload_type: u8,
    pub sequence_number: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    // pub csrc: Vec<u32>,
    // pub extension_profile: u16,
    // pub extensions: Vec<Extension>,
}

// pub struct Extension {
//     pub id: u8,
//     pub payload: Bytes,
// }

impl RTPHeader {
    pub fn serialize(&self) -> Bytes {

        let mut buf = BytesMut::with_capacity(64);

        // first byte
        let mut b0 = (self.version << 6) | 0 as u8; // this should be set to the CSRC length, but i've removed it for now
        if self.padding {
            b0 |= 1 << 5;
        }

        if self.extension {
            b0 |= 1 << 4;
        }
        buf.put_u8(b0);


        // second byte
        let mut b1 = self.payload_type;
        if self.marker {
            b1 |= 1 << 7;
        }
        buf.put_u8(b1);

        // the rest
        buf.put_u16(self.sequence_number);
        buf.put_u32(self.timestamp);
        buf.put_u32(self.ssrc);

        // for csrc in &self.csrc {
        //     buf.put_u32(*csrc);
        // }

        buf.freeze()
    }


    pub fn deserialize (packet: &mut BytesMut) -> RTPHeader {
        let b0 = packet.get_u8();
        let version = b0 >> 6 & 0x3; 
        let padding = (b0 >> 5 & 0x1) > 0;
        let extension = (b0 >> 4 & 0x1) > 0;
        // let cc = (b0 & 0xF) as usize;

        let b1 = packet.get_u8();
        let marker = (b1 >> 7 & 0x1) > 0;
        let payload_type = b1 & 0x7F;

        let sequence_number = packet.get_u16();
        let timestamp = packet.get_u32();
        let ssrc = packet.get_u32();

        // let mut csrc = Vec::with_capacity(cc);
        // for _ in 0..cc {
        //     csrc.push(packet.get_u32());
        // }

        RTPHeader {
            version,
            padding,
            extension,
            marker,
            payload_type,
            sequence_number,
            timestamp,
            ssrc,
            // csrc
        }
    }
}

pub struct RTPSession {
    pub current_sequence_num : u16,
    pub timestamp : u32,
    pub clock_rate_khz : u32,
    pub ssrc : u32
}

impl RTPSession {
    pub fn next_packet (&mut self) -> RTPHeader {

        self.current_sequence_num += 1;
        self.timestamp += self.clock_rate_khz; // roll over needed.

        RTPHeader { 
            version: 2,
            padding: false,
            extension: false,
            marker: false,
            payload_type: 0,
            sequence_number: self.current_sequence_num, 
            timestamp: self.timestamp, 
            ssrc: self.ssrc, 
            // csrc:  
        }
    }
}