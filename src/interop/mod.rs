use std::{io, slice, sync::{Arc, OnceLock}};

use tokio::{net::UdpSocket, runtime::Runtime, sync::mpsc};

use crate::session_management::peer_manager::{PeerManager, connect_to_signaling_server, run_signaling_server};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

static FRAME_TX: OnceLock<mpsc::Sender<EncodedFrame>> = OnceLock::new();
static AUDIO_TX: OnceLock<mpsc::Sender<EncodedFrame>> = OnceLock::new();

const CHANNEL_BUFFER_SIZE: usize = 64;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum StreamType {
    Audio,
    Video,
}

type ReleaseCallback = extern "C" fn(*mut std::ffi::c_void);

// sometimes reasonable men do unreasonable things
unsafe impl Send for EncodedFrame {} 

pub struct EncodedFrame  {
    data: *const u8,
    len: usize,
    context: *mut std::ffi::c_void,
    release_callback: ReleaseCallback,
}

impl Drop for EncodedFrame {
    fn drop(&mut self) {
        (self.release_callback)(self.context);
    }
}

enum UserType <'a> {
    Client(&'a str),
    Host
}

struct ServerArgs <'a> {
    stream_type : StreamType,
    user_type : UserType<'a>
}

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Runtime::new().expect("Runtime creation failed. Loser")
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_send_frame(
    data: *const u8,
    len: usize,
    stream: StreamType,
    context: *mut std::ffi::c_void,
    release_callback: ReleaseCallback
) -> bool {

    // I might switch this to getting a channel from an array.
    // -> allowing potentially infinite streams
    let tx = match stream {
        StreamType::Video => FRAME_TX.get(),
        StreamType::Audio => AUDIO_TX.get()
    };

    let tx = match tx {
        Some(tx) => tx,
        None => {
            eprintln!("Stream {:?} not initialized", stream);
            return false;
        }
    };

    // zero copy?
    let frame =  EncodedFrame {
        data,
        len,
        context,
        release_callback
    };

    match tx.try_send(frame) {
        Ok(_) => true,
        Err(mpsc::error::TrySendError::Full(_)) => {
            eprintln!("Warning: {:?} frame dropped - channel full", stream);
            false
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            eprintln!("Error: {:?} channel closed", stream);
            false
        }
    }
}


#[unsafe(no_mangle)]
pub extern "C" fn run_runtime_server (
    mut is_host: bool, 
    stream: StreamType, 
    host_addr: *const u8, 
    host_addr_len: usize
) {
    let (tx, rx) = mpsc::channel::<EncodedFrame>(CHANNEL_BUFFER_SIZE);
    
    let set_result = match stream {
        StreamType::Video => FRAME_TX.set(tx),
        StreamType::Audio => AUDIO_TX.set(tx)
    };

    if set_result.is_err() {
        eprintln!("{:?} stream already initialized", stream);
        return;
    }

    let host_addr_str = unsafe { slice::from_raw_parts(host_addr, host_addr_len) };
    let host_addr_str = str::from_utf8(host_addr_str);

    // this might be bad, but I'm just making you the host 
    // if you failed to give a correct address.
    // there's probably something wrong here too...
    let host_addr_str = match host_addr_str {
        Ok(str) => str,
        Err(_) => {
            is_host = true;
            "invalid"
        }
    };

    let server_args = ServerArgs {
        stream_type : stream,
        user_type : match is_host {
            true =>  UserType::Host,
            false => UserType::Client(host_addr_str)
        }
    };

    runtime().spawn(async move {
        if let Err(e) = network_loop_server(rx, server_args).await {
            eprintln!("Something terrible happened. Not you though. You are amazing. Always: {}", e);
        }
    });
}

async fn network_loop_server (
    rx: mpsc::Receiver<EncodedFrame>, 
    server_args : ServerArgs <'_>
) -> io::Result<()> {

    let local_addr_str = "127.0.0.1:0";

    let socket = UdpSocket::bind(local_addr_str).await?;
    let socket = Arc::new(socket);

    let peer_manager = Arc::new(PeerManager::new(socket.local_addr()?));

    match server_args.user_type {
        UserType::Client(addr) => {
            connect_to_signaling_server(addr, Arc::clone(&peer_manager), server_args.stream_type).await?
        }
        UserType::Host => {
            let peer_manager_clone = Arc::clone(&peer_manager);
        
            runtime().spawn(async move {
                if let Err(e) = run_signaling_server(peer_manager_clone, server_args.stream_type).await {
                    eprintln!("Signaling server error: {}", e);
                }
            });
        }
    }

    let sender_socket = Arc::clone(&socket);
    let sender_peers = Arc::clone(&peer_manager);

    runtime().spawn(async move {
        rtp_sender(sender_socket, sender_peers, rx).await;
    });

    rtp_receiver(socket, peer_manager).await
}

async fn rtp_sender(
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

        for addr in peers.iter() {
            match socket.send_to(data, addr).await {
                Ok(_) => {},
                Err(e) => eprintln!("Failed to send to {}: {}", addr, e),
            }
        }
    }
}

async fn rtp_receiver(
    socket: Arc<UdpSocket>,
    peer_manager: Arc<PeerManager>
) -> io::Result<()> {

    let mut buffer = [0u8; 1024];
    
    loop {
        let (bytes_read, addr) = socket.recv_from(&mut buffer).await?;

        if peer_manager.add_peer(addr).await {
            println!("new peer from: {}", addr);
        }

        print!("{}: {}", addr.to_string(), str::from_utf8(&buffer[..bytes_read]).unwrap());

        // TODO : Send to swift
    }
}