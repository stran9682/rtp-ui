use std::sync::Arc;

use bytes::{BufMut, BytesMut};
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

        let mut buffer_offset = 0;
        let block_buffer_length = frame.len;

        /*
            Taken from:
            https://stackoverflow.com/questions/28396622/extracting-h264-from-cmblockbuffer

            A frame can consist of multiple NAL units. 
            Here we are splitting them up and then sending them seperately.
         */

        // Loop through all the NAL units in the block buffer
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
                    eprintln!("Failed to convert data: {:?}", e);
                    break;
                }
            };

            // this shouldn't be possible. BUT if it is, just ignore it. Move on
            if nal_unit_length <= 0 {
                break;
            }

            //let mut buffer = BytesMut::with_capacity(nal_unit_length);
            
            let payload = &data[buffer_offset + AVCC_HEADER_LENGTH..buffer_offset + AVCC_HEADER_LENGTH + nal_unit_length];

            buffer_offset += AVCC_HEADER_LENGTH + nal_unit_length;

            println!("{}", data.len());
            println!("{:?}", header);
            println!("{}", payload.len());
            println!("{}", nal_unit_length)


        }

        for addr in peers.iter() {
            match socket.send_to(data, addr).await {
                Ok(_) => {},
                Err(e) => eprintln!("Failed to send to {}: {}", addr, e),
            }
        }
    }
}