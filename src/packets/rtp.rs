
/* 
    honestly, i've just stolen this from : 
    https://github.com/webrtc-rs/rtcp/blob/main/src/source_description/mod.rs
*/ 

use bytes::{self, Buf, BufMut, BytesMut};

pub struct Header {
    pub version: u8,
    pub padding: bool,
    pub extension: bool,
    pub marker: bool,
    pub payload_type: u8,
    pub sequence_number: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    pub csrc: Vec<u32>,
    // pub extension_profile: u16,
    // pub extensions: Vec<Extension>,
}

// pub struct Extension {
//     pub id: u8,
//     pub payload: Bytes,
// }

impl Header {
    pub fn serialize(&self) -> BytesMut {

        let mut buf = BytesMut::with_capacity(64);

        // first byte
        let mut b0 = (self.version << 6) | self.csrc.len() as u8;
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

        for csrc in &self.csrc {
            buf.put_u32(*csrc);
        }

        buf
    }


    pub fn deserialize (packet: &mut BytesMut) -> Header {
        let b0 = packet.get_u8();
        let version = b0 >> 6 & 0x3; 
        let padding = (b0 >> 5 & 0x1) > 0;
        let extension = (b0 >> 4 & 0x1) > 0;
        let cc = (b0 & 0xF) as usize;

        let b1 = packet.get_u8();
        let marker = (b1 >> 7 & 0x1) > 0;
        let payload_type = b1 & 0x7F;

        let sequence_number = packet.get_u16();
        let timestamp = packet.get_u32();
        let ssrc = packet.get_u32();

        let mut csrc = Vec::with_capacity(cc);
        for _ in 0..cc {
            csrc.push(packet.get_u32());
        }

        Header {
            version,
            padding,
            extension,
            marker,
            payload_type,
            sequence_number,
            timestamp,
            ssrc,
            csrc
        }
    }
}

pub fn add_payload (
    mut header : BytesMut,
    payload : &[u8],
    packet_type : FragmentedPacket
) -> BytesMut {

    /*
        +---------------+---------------+
        |0|1|2|3|4|5|6|7|0|1|2|3|4|5|6|7|
        +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
        |F|NRI|  Type   |S|E|R|  Type   |
        +---------------+---------------+

        F           : should be 0 
        NRI         : Essentialy level of importance, needs to be copied
        Type (1)    : Type of header. 28 To indicate this is a fragment
        S(tart)     : indicates this is the start
        E(nd)       : indicates this is the end
        R(eserved)  : always 0
        Type (2)    : Kind of payload, needs to be copied

        Original header needs to be reconstructed!
     */

    let nalu_nri = payload[0] & 0x60;
    let nalu_type = payload[0] & 0x1F;

    let b0 = 28 | nalu_nri; // 28 to indicate FU-A packet type
    header.put_u8(b0);

    let mut b1 = nalu_type;
    
    match packet_type {
        FragmentedPacket::End => {
            b1 |= 1 << 7;
        }
        FragmentedPacket::Start => {
            b1 |= 1 << 6;
        }
        _ => ()
    }

    header.put_u8(b1);

    header.put(payload);

    header
}

pub enum FragmentedPacket {
    Start,
    End,
    Other
}

