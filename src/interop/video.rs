use std::sync::OnceLock;
use std::{io, sync::Arc};
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::{BufMut, Bytes, BytesMut};
use tokio::{net::UdpSocket, sync::mpsc};

use crate::packets::rtp::RTPHeader;
use crate::session_management::peer_manager::{Fragment, PlayoutBufferNode};
use crate::{packets::rtp::RTPSession, session_management::peer_manager::PeerManager};

static FRAME_OUTPUT: OnceLock<Arc<PeerManager>> = OnceLock::new();

const AVCC_HEADER_LENGTH: usize = 4;

pub type ReleaseCallback = extern "C" fn(*mut std::ffi::c_void);

pub struct EncodedFrame  {
    pub data: *const u8,
    pub len: usize,
    pub context: *mut std::ffi::c_void,
    pub release_callback: ReleaseCallback,
}

impl Drop for EncodedFrame {
    fn drop(&mut self) {
        (self.release_callback)(self.context);
    }
}

// sometimes reasonable men do unreasonable things
unsafe impl Send for EncodedFrame {} 

pub async fn rtp_frame_sender(
    socket: Arc<UdpSocket>,
    peer_manager: Arc<PeerManager>,
    mut rx: mpsc::Receiver<EncodedFrame>
) {    

    let mut rtp_session = RTPSession{
        current_sequence_num: 0,
        timestamp: 0,
        increment: 3_000,
        ssrc: 1
    };

    loop {

        let frame = match rx.recv().await {
            Some(f) => f,
            None => break,
        };

        let peers = peer_manager.get_peers();
        
        if peers.is_empty() {
            continue;
        }

        // construct the slice on the SPOT!
        let data = unsafe {
            std::slice::from_raw_parts(frame.data, frame.len)
        };

        let nal_units = get_nal_units(data);
        let mut nal_units = nal_units.iter().peekable();

        while let Some(nal_unit) = nal_units.next() {
            let fragments = get_fragments(nal_unit, &mut rtp_session, nal_units.peek().is_none());

            for fragment in fragments {

                for addr in peers.iter() {
                    match socket.send_to(&fragment, addr).await {
                        Ok(_) => {},
                        Err(e) => eprintln!("Failed to send to {}: {}", addr, e),
                    }
                }
            }
        }

        rtp_session.next_packet(); // this will increment the timestamp by 3000. (90kHz / 30 fps)
    }
}

pub fn get_fragments(payload : &[u8], rtp_session : &mut RTPSession, is_last_unit: bool) -> Vec<Bytes> {
    let mut payloads = Vec::new();

    let max_fragment_size = 1200; // low key a magic number...
    let mut nalu_data_index = 1;
    let nalu_data_length = payload.len() - nalu_data_index; 
    let mut nalu_data_remaining = nalu_data_length;

    let nalu_nri = payload[0] & 0x60;
    let nalu_type = payload[0] & 0x1F;

    if payload.len() <= max_fragment_size {

        let rtp_header = rtp_session.get_packet(is_last_unit);

        let rtp_header = rtp_header.serialize();

        let mut out = BytesMut::with_capacity(payload.len() + rtp_header.len());

        out.put(rtp_header);
        out.put(payload);

        payloads.push(out.freeze());
        return payloads;
    }

    while nalu_data_remaining > 0 {

        let current_fragment_size = std::cmp::min(max_fragment_size, nalu_data_remaining);

        let rtp_header = rtp_session.get_packet(
            is_last_unit && max_fragment_size >= nalu_data_remaining // VERY last one
        ).serialize(); // this will move the sequence number by 1

        let mut out = BytesMut::with_capacity(2 + current_fragment_size + rtp_header.len());

        out.put_slice(&rtp_header);

        /*
            +---------------+---------------+
            |0|1|2|3|4|5|6|7|0|1|2|3|4|5|6|7|
            +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
            |F|NRI|  Type   |S|E|R|  Type   |
            +---------------+---------------+

            F           : should always be 0 
            NRI         : Essentialy level of importance, needs to be copied
            Type (1)    : Type of header. 28 To indicate this is a fragment
            S(tart)     : indicates this is the start
            E(nd)       : indicates this is the end
            R(eserved)  : always 0
            Type (2)    : Kind of payload, needs to be copied

            Original header needs to be reconstructed!
        */

        let b0 = 28 | nalu_nri; // 28 to indicate FU-A packet type
        out.put_u8(b0);

        let mut b1 = nalu_type;
            if nalu_data_remaining == nalu_data_length {
            // Set start bit
            b1 |= 1 << 7;
        } else if nalu_data_remaining - current_fragment_size == 0 {
            // Set end bit
            b1 |= 1 << 6;
        }
        out.put_u8(b1);
        
        out.put_slice(&payload[nalu_data_index..nalu_data_index + current_fragment_size]);

        nalu_data_remaining -= current_fragment_size;
        nalu_data_index += current_fragment_size;

        payloads.push(out.freeze());
    }

    payloads
}

pub fn get_nal_units(data: &[u8]) -> Vec<&[u8]> {

    println!("{}", data.len());

    let mut nal_units = Vec::new();

    /*
        Taken from:
        https://stackoverflow.com/questions/28396622/extracting-h264-from-cmblockbuffer

        A frame can consist of multiple NAL units. 
        Here we are splitting them up and then sending them seperately.
    */

    // Loop through all the NAL units in the block buffer

    let mut buffer_offset = 0;
    let block_buffer_length = data.len();

    while buffer_offset < (block_buffer_length - AVCC_HEADER_LENGTH) {

        // Read the NAL unit length   
        let header = &data[buffer_offset..buffer_offset + AVCC_HEADER_LENGTH];

        let header: [u8; 4] = match header.try_into(){
            Ok(arr) => arr,
            Err(e) => {
                eprintln!("Failed to get length of data: {:?}", e);
                break;
            }
        };

        let nal_unit_length : i32 = i32::from_be_bytes(header);

        let nal_unit_length: usize = match nal_unit_length.try_into() {
            Ok(res) => res,
            Err(e) => {
                eprintln!("Failed to convert data from i32 to usize: {:?}", e);
                break;
            }
        };

        // this shouldn't be possible. BUT if it is, just ignore it. Move on
        if nal_unit_length == 0 {
            break;
        }
        
        let payload = &data[buffer_offset + AVCC_HEADER_LENGTH..buffer_offset + AVCC_HEADER_LENGTH + nal_unit_length];

        nal_units.push(payload);

        buffer_offset += AVCC_HEADER_LENGTH + nal_unit_length;

        // println!("{}", data.len());
        // println!("{:?}", header);
        // println!("{}", payload.len());
        // println!("{}", nal_unit_length);
                    
    }

    nal_units
}

pub fn rtp_to_avcc_h264 (packets : Vec<Bytes>) -> *const u8{
    let mut payload = BytesMut::new();
    let mut fua_buffer = BytesMut::new();

    let b0 = packets[0][0];
    let nalu_type = b0 & 0x1F;

    for packet in packets {
        match nalu_type {
            1..=23 => {
                payload.put_u32(packet.len() as u32);
                payload.put(packet);
            }

            // lala skip a few. Shouldn't need these!!

            28 => {
                fua_buffer.put(packet.slice(2 as usize..)); // just payload, skip the header.

                let b1 = packet[1];

                if b1 & 0x40 != 0 {
                    let nalu_ref_idc = b0 & 0x60;
                    let fragmented_nalu_type = b1 & 0x1F;

                    payload.put_u32((fua_buffer.len() + 1) as u32);
                    
                    payload.put_u8(nalu_ref_idc | fragmented_nalu_type);
                    payload.put(fua_buffer);

                    fua_buffer = BytesMut::new(); // real dirty, I know... clears the buffer if there's any other fua packets.
                }
            }

            _ => () // erm
        }
    };

    return payload.freeze().as_ptr()
}

pub async fn rtp_frame_receiver(
    socket: Arc<UdpSocket>,
    peer_manager: Arc<PeerManager>,
    media_clock_rate: u32
) -> io::Result<()> {

    let mut buffer = [0u8; 1500];

    let _ = FRAME_OUTPUT.set(Arc::clone(&peer_manager));
    
    loop {
        let (bytes_read, addr) = socket.recv_from(&mut buffer).await?;

        // there's absolutely a bug where if the time switches playout will be messed up!
        // (ex: when there's daylight savings)
        // but the wall clock is "technically" more stable, and less susceptible to skew
        // bet big, take risks, that's the way.

        let now = SystemTime::now();

        let duration_since = now
            .duration_since(UNIX_EPOCH);

        let duration_since = match duration_since {
            Ok(yay) => yay,
            Err(_) => {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "holy what happened??"));
            }
        };

        /*
            Calculating Base Playout time:

            M = T * R + offset
            d(n) = Arrival Time of Packet - Header Timestamp
            offset = Min(d(n-w)...d(n))
            base playout time = Timestamp + offset

         */

        let arrival_time = (duration_since.as_millis() * media_clock_rate as u128) / 1000;

        let mut data = BytesMut::with_capacity(bytes_read);
        data.put_slice(&buffer[..bytes_read]);

        let header = RTPHeader::deserialize(&mut data);

        let difference = arrival_time - header.timestamp as u128;

        let offset = peer_manager.peer_get_min_window(addr, difference);

        let base_playout_time = header.timestamp as u128 + offset;

        let node = PlayoutBufferNode {
            arrival_time,
            rtp_timestamp : header.timestamp,
            playout_time : base_playout_time,
            coded_data : Vec::new()
        };

        let fragment = Fragment {
            sequence_num: header.sequence_number,
            data: data.freeze()
        };

        peer_manager.add_playout_node_to_peer(addr, node, fragment);

        println!("{}: {}", addr.to_string(), str::from_utf8(&buffer[..bytes_read]).unwrap());

        // TODO : Send to swift
    }
}