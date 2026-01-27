use core::slice;
use std::{collections::HashSet, net::SocketAddr, sync::{Arc, OnceLock}};
use bytes::{BufMut, Bytes, BytesMut};
use tokio::{io::{self, AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream }, sync::{Mutex, OnceCell}};

use crate::{interop::{StreamType, runtime}, session_management::peer_manager::PeerManager};

const BUFFER_SIZE: usize = 1500;

static AUDIO_PEERS: OnceLock<Arc<PeerManager>> = OnceLock::new();
static FRAME_PEERS: OnceLock<Arc<PeerManager>> = OnceLock::new();
static LISTENER: OnceCell<TcpListener> = OnceCell::const_new();


struct H264Args{
    sps: Bytes,
    pps: Bytes,
}

struct PeerSpecifications {
    peer_signaling_address : Mutex<HashSet<SocketAddr>>,
    self_h264_args : H264Args
}

impl PeerSpecifications {
    pub fn new (pps: Bytes, sps: Bytes) -> Self {
        Self {
            peer_signaling_address : Mutex::new(HashSet::new()),
            self_h264_args: H264Args { sps, pps }
        }
    }

    pub async fn get_peers(&self) -> HashSet<SocketAddr> {
        self.peer_signaling_address.lock().await.clone()
    }

    pub async fn add_peer(&self, addr: SocketAddr) {
        let mut peers = self.peer_signaling_address.lock().await;

        peers.insert(addr);
    }
}

static PEER_SPECIFICATIONS : OnceLock<PeerSpecifications> = OnceLock::new();

pub extern "C" fn rust_send_h264_config (
    pps: *const u8,
    pps_length: usize,
    sps: *const u8,
    sps_length: usize,
    host_addr: *const u8,
    host_addr_length: usize
) {
    let host_addr_str = if host_addr.is_null() {
        None
    } else {
        let host_addr_slice = unsafe {
            slice::from_raw_parts(host_addr, host_addr_length)
        };

        let Ok(host_addr_str) = str::from_utf8(host_addr_slice) else {
            return;
        };

        Some(host_addr_str)
    };
    
    let pps = unsafe {
        slice::from_raw_parts(pps, pps_length)
    };

    let pps = Bytes::copy_from_slice(pps);

    let sps = unsafe {
        slice::from_raw_parts(sps, sps_length)
    };

    let sps = Bytes::copy_from_slice(sps);

    let _ = PEER_SPECIFICATIONS.set(PeerSpecifications::new(pps, sps));

    let frame_peer_clone = Arc::clone(&FRAME_PEERS.get().unwrap());
    runtime().spawn(async move {
        connect_to_signaling_server(host_addr_str, frame_peer_clone, StreamType::Video)
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

    loop {
        let (mut socket, client_addr) = match listener().await.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
                continue;
            }
        }; 

        // just twaddle until we get our own specs, awaiting a connection should hold this off
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

    let remote_addr_str = std::str::from_utf8(&buffer[..bytes_read])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    
    // split 
    let request: Vec<&str> = remote_addr_str
        .lines()
        .take_while(|line| !line.is_empty())
        .collect();

    let (stream_type, peer_manager) = match request[0] {
        "video" => (StreamType::Video,  FRAME_PEERS.get()),
        "audio" => (StreamType::Audio, AUDIO_PEERS.get()),
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "Not a valid type"))
    };

    let Some(peer_manager) = peer_manager else {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Peer manager not initialized"));
    };

    let mut response = BytesMut::new();

    let header = format!("{}\r\n{}\r\n", request[0], peer_manager.local_addr);
    response.put(header.as_bytes());

    let Some(specifications) = PEER_SPECIFICATIONS.get() else {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Specification manager not initialized"));
    };

    match stream_type {
        StreamType::Video => {        
            response.put_slice(&specifications.self_h264_args.pps);
            response.put_slice(b"\r\n");
            response.put_slice(&specifications.self_h264_args.sps);
            
            let signaling = specifications.get_peers().await;
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

    let signaling_addr: SocketAddr = request[1]
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    specifications.add_peer(signaling_addr).await;

    let media_addr: SocketAddr = request[2]
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    peer_manager.add_peer(media_addr).await;

    // TODO: Update UI

   Ok(()) 
}

async fn connect_to_signaling_server(
    server_addr: Option<&str>,
    peer_manager: Arc<PeerManager>,
    stream_type : StreamType
) -> io::Result<()> {

    // this is the case when you're the first person. 
    // You don't have anyone to connect to
    let Some(server_addr) = server_addr else {
        return Ok(());
    };

    let mut socket = TcpStream::connect(server_addr).await?;   

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

    socket.write_all(&packet).await?;

    let mut buffer = [0u8; BUFFER_SIZE];
    let bytes_read = socket.read(&mut buffer).await?;
    
    if bytes_read == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "No response from server"));
    }

    let responses = str::from_utf8(&buffer[..bytes_read])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let responses: Vec<&str> = responses
        .lines()
        .take_while(|line| !line.is_empty())
        .collect();

    let media_addr: SocketAddr = responses[1]
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    peer_manager.add_peer(media_addr).await;

    for signaling_addr in &responses[4..] {
        let signaling_addr: SocketAddr = signaling_addr
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        PEER_SPECIFICATIONS.get().unwrap().add_peer(signaling_addr).await;
    }

    // TODO: update swift ui!

    Ok(())
}
