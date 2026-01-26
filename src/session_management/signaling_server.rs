use core::slice;
use std::{collections::HashMap, net::SocketAddr, sync::{Arc, OnceLock}};
use bytes::{BufMut, Bytes, BytesMut};
use tokio::{io::{self, AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream }, sync::{Mutex, OnceCell}};

use crate::{interop::{StreamType, runtime}, session_management::peer_manager::PeerManager};

const BUFFER_SIZE: usize = 1500;

static AUDIO_PEERS: OnceLock<Arc<PeerManager>> = OnceLock::new();
static FRAME_PEERS: OnceLock<Arc<PeerManager>> = OnceLock::new();
static LISTENER: OnceCell<TcpListener> = OnceCell::const_new();


struct H264Args{
    sps: Bytes,
    pps: Bytes
}

enum SignalingServerArgs {
    Video(H264Args),
    Audio
}

pub extern "C" fn rust_send_h264_config (
    pps: *const u8,
    pps_length: usize,
    sps: *const u8,
    sps_length: usize,
    host_addr: *const u8,
    host_addr_length: usize
) {
    let host_addr_slice = unsafe {
        slice::from_raw_parts(host_addr, host_addr_length)
    };

    let Ok(host_addr_str) = str::from_utf8(host_addr_slice) else {
        return;
    };

    let pps = unsafe {
        slice::from_raw_parts(pps, pps_length)
    };

    let pps = Bytes::copy_from_slice(pps);

    let sps = unsafe {
        slice::from_raw_parts(sps, sps_length)
    };

    let sps = Bytes::copy_from_slice(sps);

    let h264_args = H264Args {
        sps,
        pps
    };

    let frame_peer_clone = Arc::clone(&FRAME_PEERS.get().unwrap());
    runtime().spawn(async move {
        connect_to_signaling_server(host_addr_str, frame_peer_clone, SignalingServerArgs::Video(h264_args))
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

        println!("Request from {}", client_addr.to_string());

        tokio::spawn(async move {
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

    let stream_type = match request[1] {
        "1" => StreamType::Video,
        "0" => StreamType::Audio,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "Not a valid type"))
    };

    let remote_addr: SocketAddr = request[0]
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let peer_manager = match stream_type {
        StreamType::Audio => AUDIO_PEERS.get(),
        StreamType::Video => FRAME_PEERS.get()
    };

    let peer_manager = match peer_manager {
        Some(peer_manager) => peer_manager,
        None => {
            return Err(io::Error::new(io::ErrorKind::NotFound, "Peer manager not initialized"));
        }
    };

    let is_new = peer_manager.add_peer(remote_addr).await;
    
    if !is_new {
        return Ok(());
    }

    let mut response = BytesMut::with_capacity(BUFFER_SIZE);

    let header = format!("
        v=0\r\n
        o={}\r\n
        s={}\r\n
        m={}\r\n
        a=", 
        
        listener().await.local_addr()?,
        match stream_type {
            StreamType::Audio => "Audio",
            StreamType::Video => "Video"
        },
        peer_manager.local_addr
    );

    response.put_slice(header.as_bytes());

    let peers = peer_manager.get_peers().await;

    for addr in peers.iter() {
        if *addr != remote_addr {
            response.put_slice(b"\r\n");
            response.put_slice(addr.to_string().as_bytes());
            response.put("\r\n".as_bytes());

        }
    }

    socket.write_all(&response).await?;

    // TODO: Update UI

   Ok(()) 
}

async fn connect_to_signaling_server(
    server_addr: &str,
    peer_manager: Arc<PeerManager>,
    stream_type : SignalingServerArgs
) -> io::Result<()> {
    let mut socket = TcpStream::connect(server_addr).await?;
    
    match stream_type {
        SignalingServerArgs::Audio => {
            let str = peer_manager.local_addr.to_string() + "\r\n" + "0";

            socket.write_all(str.as_bytes()).await?;
        },
        SignalingServerArgs::Video(args) => {
            let mut packet = BytesMut::new();

            let header = format!("
                v=0\r\n
                o={}\r\n
                s=video\r\n
                m={}\r\n
                a=", 
                
                listener().await.local_addr()?,
                peer_manager.local_addr
            );

            packet.put(header.as_bytes());

            packet.put(args.pps);

            packet.put_slice(b",");            

            packet.put(args.sps);

            packet.put_slice(b"\r\n");

            socket.write_all(&packet).await?;
        }
    };   
    
    let mut buffer = [0u8; BUFFER_SIZE];
    let bytes_read = socket.read(&mut buffer).await?;
    
    if bytes_read == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "No response from server"));
    }

    let response = str::from_utf8(&buffer[..bytes_read])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;


    // for line in response.lines() {
    //     if let Ok(addr) = line.parse::<SocketAddr>() {
           
    //         let res = peer_manager.add_peer(addr).await;

    //         println!("{}, {}", addr.to_string(), res);
    //     }
    // }

    // TODO: update swift ui!

    Ok(())
}
