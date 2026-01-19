use std::sync::Arc;

use bytes::{BufMut, Bytes, BytesMut};
use tokio::{net::UdpSocket, sync::mpsc};

use crate::session_management::peer_manager::PeerManager;

const AVCC_HEADER_LENGTH: usize = 4;

#[repr(C)]
pub enum FrameType {
    Pps,
    Sps,
    Keyframe,
    Other
}

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
    loop {

        let frame = match rx.recv().await {
            Some(f) => f,
            None => break,
        };

        let peers = peer_manager.get_peers().await;
        
        if peers.is_empty() {
            continue;
        }

        // construct the slice on the SPOT!
        let data = unsafe {
            std::slice::from_raw_parts(frame.data, frame.len)
        };

        let nal_units = get_nal_units(data);

        for nal_unit in nal_units {
            let fragments = get_fragments(nal_unit);

            for fragment in fragments {
                
                for addr in peers.iter() {
                    match socket.send_to(&fragment, addr).await {
                        Ok(_) => {},
                        Err(e) => eprintln!("Failed to send to {}: {}", addr, e),
                    }
                }
            }
        }   
    }
}


fn get_fragments(payload : &[u8]) -> Vec<Bytes> {
    let mut payloads = Vec::new();

    let max_fragment_size = 1200;
    let mut nalu_data_index = 1;
    let nalu_data_length = payload.len() - nalu_data_index; 
    let mut nalu_data_remaining = nalu_data_length;

    let nalu_nri = payload[0] & 0x60;
    let nalu_type = payload[0] & 0x1F;

    while nalu_data_remaining > 0 {

        let current_fragment_size = std::cmp::min(max_fragment_size, nalu_data_remaining);

        let mut out = BytesMut::with_capacity(2 + current_fragment_size);

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

fn get_nal_units(data: &[u8]) -> Vec<&[u8]> {

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
        if nal_unit_length <= 0 {
            break;
        }
        
        let payload = &data[buffer_offset + AVCC_HEADER_LENGTH..buffer_offset + AVCC_HEADER_LENGTH + nal_unit_length];

        nal_units.push(payload);

        buffer_offset += AVCC_HEADER_LENGTH + nal_unit_length;

        println!("{}", data.len());
        println!("{:?}", header);
        println!("{}", payload.len());
        println!("{}", nal_unit_length);
                    
    }

    nal_units
}