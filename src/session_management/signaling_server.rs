use core::slice;
use std::{collections::HashSet, net::SocketAddr, sync::{Arc, OnceLock}};
use bytes::{BufMut, Bytes, BytesMut};
use dashmap::DashSet;
use tokio::{io::{self, AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream }, sync::OnceCell};

use crate::{interop::{StreamType, runtime}, session_management::peer_manager::PeerManager};

const BUFFER_SIZE: usize = 1500;

static AUDIO_PEERS: OnceLock<Arc<PeerManager>> = OnceLock::new();
static FRAME_PEERS: OnceLock<Arc<PeerManager>> = OnceLock::new();
static LISTENER: OnceCell<TcpListener> = OnceCell::const_new();
static VIDEO_CONTEXT: OnceLock<SpsPpsContext> = OnceLock::new();

pub type SpsPpsCallback = extern "C" fn(
    context: *mut std::ffi::c_void, 
    pps: *const u8, 
    pps_length: usize, 
    sps: *const u8, 
    sps_length: usize
);

struct SpsPpsContext {
    context: *mut std::ffi::c_void,
    callback: SpsPpsCallback
}

// BAD BAD BAD!
unsafe impl Send for SpsPpsContext { }
unsafe impl Sync for SpsPpsContext { }

struct H264Args{
    sps: Bytes,
    pps: Bytes,
}

pub struct PeerSpecifications {
    peer_signaling_address : DashSet<SocketAddr>,
    self_h264_args : H264Args
}

impl PeerSpecifications {
    pub fn new (pps: Bytes, sps: Bytes) -> Self {
        Self {
            peer_signaling_address : DashSet::new(),
            self_h264_args: H264Args { sps, pps }
        }
    }

    pub fn get_peers(&self) -> HashSet<SocketAddr> {
        self.peer_signaling_address.iter().map(|addr| addr.clone()).collect()
    }

    pub fn add_peer(&self, addr: SocketAddr) {
        self.peer_signaling_address.insert(addr);
    }
}

pub static PEER_SPECIFICATIONS : OnceLock<PeerSpecifications> = OnceLock::new();
static SIGNALLING_ADDR: OnceLock<String> = OnceLock::new();

#[unsafe(no_mangle)]
pub extern "C" fn rust_set_signalling_addr(
    host_addr: *const u8,
    host_addr_length: usize
) { 
    if !host_addr.is_null() {
        let host_addr_slice = unsafe {
            slice::from_raw_parts(host_addr, host_addr_length)
        };

        let Ok(host_addr_str) = str::from_utf8(host_addr_slice) else {
            return;
        };

        let _ = SIGNALLING_ADDR.set(host_addr_str.to_string());

        println!("Set address!, {}", SIGNALLING_ADDR.get().unwrap())
    } 
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_send_video_callback (context: *mut std::ffi::c_void, callback: SpsPpsCallback){
    let _ = VIDEO_CONTEXT.set(SpsPpsContext { context, callback });
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_send_h264_config (
    pps: *const u8,
    pps_length: usize,
    sps: *const u8,
    sps_length: usize,
) {
 
    let pps = unsafe {
        slice::from_raw_parts(pps, pps_length)
    };

    let pps = Bytes::copy_from_slice(pps);

    let sps = unsafe {
        slice::from_raw_parts(sps, sps_length)
    };

    let sps = Bytes::copy_from_slice(sps);

    let _ = PEER_SPECIFICATIONS.set(PeerSpecifications::new(pps, sps));

    let host_addr_str = match SIGNALLING_ADDR.get() {
        Some(addr) => Some(addr.to_owned()),
        None => None
    };

    let Some(frame_peers) = FRAME_PEERS.get() else {
        eprintln!("Frame peer manager not set yet, have you called run_runtime_server first?");
        return;
    };

    let frame_peer_clone = Arc::clone(&frame_peers);
    runtime().spawn(async move {
        if let Err(e) = connect_to_signaling_server(host_addr_str, frame_peer_clone, StreamType::Video).await {
            eprintln!("Failed to connect to signaling server, {}", e)
        }
    });
}

async fn listener() -> &'static TcpListener {
    LISTENER.get_or_init(|| async {
        TcpListener::bind("0.0.0.0:0").await.unwrap()
    }).await
}

// inject an instance of a peer manager for the server to manage
pub async fn run_signaling_server (
    peer_manager : Arc<PeerManager>,
    stream_type : StreamType
) -> io::Result<()> {

    let res = match stream_type {
        StreamType::Audio => AUDIO_PEERS.set(Arc::clone(&peer_manager)),
        StreamType::Video => {
            FRAME_PEERS.set(Arc::clone(&peer_manager))
        }
    };

    // return early. Do NOT run another instance of the server!
    if res.is_err() || 
        (!AUDIO_PEERS.get().is_none() && !FRAME_PEERS.get().is_none()) {
        return Ok(()); 
    }

    println!("{}", listener().await.local_addr().unwrap());

    loop {
        let (mut socket, client_addr) = match listener().await.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
                continue;
            }
        }; 

        // just twaddle until we get our own specs, 
        // awaiting a connection should hold this off
        if PEER_SPECIFICATIONS.get().is_none() { continue; } 

        println!("Request from {}", client_addr.to_string());

        runtime().spawn(async move {
            if let Err(e) = handle_signaling_client(&mut socket).await {
                eprintln!("Signaling error with {}: {}", client_addr, e);
            }
        });
    }
}

async fn handle_signaling_client (
    socket : &mut TcpStream, 
) -> io::Result<()> {
    let mut buffer = [0; BUFFER_SIZE];

    let bytes_read = socket.read(&mut buffer).await?;
    if bytes_read == 0 {
        return Ok(());
    }

    let data = Bytes::copy_from_slice(&buffer[..bytes_read]);

    let request: Vec<&[u8]> = data
        .split(|b| b == &0xA)
        .map(|line| line.strip_suffix(&[0xD])
        .unwrap_or(line))
        .collect();

    let Ok(media_type) = str::from_utf8(request[0]) else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Unable to parse media type"));
    };

    let (stream_type, peer_manager) = match media_type {
        "video" => (StreamType::Video,  FRAME_PEERS.get()),
        "audio" => (StreamType::Audio, AUDIO_PEERS.get()),
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "Not a valid type"))
    };

    let Some(peer_manager) = peer_manager else {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Peer manager not initialized"));
    };

    let mut response = BytesMut::new();

    let header = format!("{}\r\n{}\r\n", media_type, peer_manager.local_addr);
    response.put(header.as_bytes());

    let Some(specifications) = PEER_SPECIFICATIONS.get() else {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Specification manager not initialized"));
    };

    match stream_type {
        StreamType::Video => {        
            response.put_slice(&specifications.self_h264_args.pps);
            response.put_slice(b"\r\n");
            response.put_slice(&specifications.self_h264_args.sps);
            
            let signaling = specifications.get_peers();
            for addr in signaling {
                response.put_slice(b"\r\n");
                response.put(addr.to_string().as_bytes());
            }
        },
        StreamType::Audio => {
            // STILL WORKING ON IT!
        }
    }

    socket.write_all(&response).await?;

    let signaling_addr: SocketAddr = str::from_utf8(request[1])
        .map_err(|e| io::Error::new(
            io::ErrorKind::InvalidData, 
            format!("Somemone sent you a faulty signaling address. {}", e)))?
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let media_addr: SocketAddr = str::from_utf8(request[2])
        .map_err(|e| io::Error::new(
            io::ErrorKind::InvalidData, 
            format!("Somemone sent you a faulty media address. {}", e)))?
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    peer_manager.add_peer(media_addr);
    specifications.add_peer(signaling_addr);

    let context = VIDEO_CONTEXT.get().unwrap();

    (context.callback)(context.context, request[3].as_ptr(), request[3].len(), request[4].as_ptr(), request[4].len());

   Ok(()) 
}

pub async fn connect_to_signaling_server(
    server_addr: Option<String>,
    peer_manager: Arc<PeerManager>,
    stream_type : StreamType
) -> io::Result<()> {

    // this is the case when you're the first person. 
    // You don't have anyone to connect to
    let Some(server_addr) = server_addr else {
        return Ok(());
    };

    let mut packet = BytesMut::new();

    match stream_type {
        StreamType::Audio => packet.put_slice(b"audio\r\n"),
        StreamType::Video => packet.put_slice(b"video\r\n")
    }

    let Ok(signaling_addr) = listener().await.local_addr() else {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Failed to get signaling address"));
    };

    packet.put(signaling_addr.to_string().as_bytes());
    packet.put_slice(b"\r\n");
    packet.put(peer_manager.local_addr.to_string().as_bytes());
    packet.put_slice(b"\r\n");

    let Some(peer_specs) = PEER_SPECIFICATIONS.get() else {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted, 
            "Peer Specifications object not intialized. Most likely missing PPS and SPS data")
        );
    };

    match stream_type {
        StreamType::Audio => {
            // STILL WORKING ON IT!
        }
        StreamType::Video => {
            packet.put_slice(&peer_specs.self_h264_args.pps);
            packet.put_slice(b"\r\n");
            packet.put_slice(&peer_specs.self_h264_args.sps);
        }
    }

    let mut addresses: Vec<String> = Vec::new();
    add_peers(&peer_manager, &server_addr, &packet, &mut addresses).await?;

    for signaling_addr in &addresses {
        // throwaway vector lol. don't do this.
        if let Err(e) = add_peers(&peer_manager, signaling_addr, &packet, &mut Vec::new()).await {
            eprint!("Error! : {}", e);
            continue;
        }
    }

    Ok(())
}

async fn add_peers (peer_manager: &Arc<PeerManager>, signaling_addr: &str, packet: &BytesMut, addresses: &mut Vec<String>) -> io::Result<()> {
    let mut buffer = [0u8; BUFFER_SIZE];
    let mut socket = TcpStream::connect(signaling_addr).await?;
    
    socket.write_all(&packet).await?;
    
    let bytes_read = socket.read(&mut buffer).await?;

    if bytes_read == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "No response from server"));
    }

    let data = Bytes::copy_from_slice(&buffer[..bytes_read]);   

    let data: Vec<&[u8]> = data
        .split(|b| b == &0xA)
        .map(|line| line.strip_suffix(&[0xD])
        .unwrap_or(line))
        .collect();

    let media_addr: SocketAddr = str::from_utf8(data[1])
        .map_err(|e| io::Error::new(
            io::ErrorKind::AddrNotAvailable, 
            format!("Failed to prase address, {}", e)))?
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    peer_manager.add_peer(media_addr);

    let signaling_addr: SocketAddr = signaling_addr
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    PEER_SPECIFICATIONS.get().unwrap().add_peer(signaling_addr);

    for response in &data[4..] {
        let Ok(str) = str::from_utf8(response) else {
            continue;
        };
        
        addresses.push(str.to_string());
    }

    let context = VIDEO_CONTEXT.get().unwrap();

    (context.callback)(context.context, data[2].as_ptr(), data[2].len(), data[3].as_ptr(), data[3].len());
    Ok(())
}