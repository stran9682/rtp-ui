pub mod video;
pub mod audio;

use std::{io::{self}, sync::{Arc, OnceLock}};

use tokio::{net::UdpSocket, runtime::Runtime, sync::mpsc};

use crate::{interop::{audio::{EncodedAudio, rtp_audio_receiver}, video::{EncodedFrame, ReleaseCallback, rtp_frame_receiver, rtp_frame_sender}}, session_management::{peer_manager::PeerManager, signaling_server::run_signaling_server}};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

static FRAME_TX: OnceLock<mpsc::Sender<EncodedFrame>> = OnceLock::new();
static AUDIO_TX: OnceLock<mpsc::Sender<EncodedAudio>> = OnceLock::new();

const CHANNEL_BUFFER_SIZE: usize = 64;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum StreamType {
    Audio,
    Video,
}

pub fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Runtime::new().expect("Runtime creation failed. Loser")
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_send_frame(
    data: *const u8,
    len: usize,
    context: *mut std::ffi::c_void,
    release_callback: ReleaseCallback
) -> bool {

    let tx = match FRAME_TX.get() {
        Some(tx) => tx,
        None => {
            eprintln!("Video stream not initialized");
            return false;
        }
    };

    // zero copy?
    let frame =  EncodedFrame {
        data,
        len,
        context,
        release_callback,
    };

    match tx.try_send(frame) {
        Ok(_) => true,
        Err(mpsc::error::TrySendError::Full(_)) => {
            eprintln!("Warning: frame dropped - channel full");
            false
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            eprintln!("Error: channel closed");
            false
        }
    }
}


#[unsafe(no_mangle)]
pub extern "C" fn run_runtime_server ( 
    stream: StreamType
) {
    runtime().spawn(async move {
        if let Err(e) = network_loop_server(stream).await {
            eprintln!("Something terrible happened. Not you though. You are amazing. Always: {}", e);
        }
    });
}

async fn network_loop_server (
    stream_type: StreamType
) -> io::Result<()> {

    let local_addr_str = "127.0.0.1:0";

    let socket = UdpSocket::bind(local_addr_str).await?;
    let socket = Arc::new(socket);

    let peer_manager = Arc::new(PeerManager::new(socket.local_addr()?));

    let peer_manager_clone = Arc::clone(&peer_manager);        
    runtime().spawn(async move {
        if let Err(e) = run_signaling_server(peer_manager_clone, stream_type).await {
            eprintln!("Signaling server error: {}", e);
        }
    });


    let sender_socket = Arc::clone(&socket);
    let sender_peers = Arc::clone(&peer_manager);
    
    match stream_type {
        StreamType::Video => {
            let (tx, rx) = mpsc::channel::<EncodedFrame>(CHANNEL_BUFFER_SIZE);

            FRAME_TX.set(tx).map_err(|_| {
                eprintln!("{:?} stream already initialized", stream_type);
                return io::Error::new(io::ErrorKind::AlreadyExists, "video stream already in use");
            })?;

            runtime().spawn(async move {
                rtp_frame_sender(sender_socket, sender_peers, rx).await;
            });

            rtp_frame_receiver(socket, peer_manager, 90_000).await
        },

        StreamType::Audio => {
            let (tx, _rx) = mpsc::channel::<EncodedAudio>(CHANNEL_BUFFER_SIZE);

            AUDIO_TX.set(tx).map_err(|_| {
                eprintln!("{:?} stream already initialized", stream_type);
                return io::Error::new(io::ErrorKind::AlreadyExists, "audio stream already in use");
            })?;

            // TODO : spawn an audio runtime.
            rtp_audio_receiver().await
        }
    }
}