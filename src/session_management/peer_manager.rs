use std::{net::SocketAddr, sync::{Arc, OnceLock}};
use bytes::{BufMut, BytesMut};
use tokio::{io::{self, AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream }, sync::{Mutex, OnceCell}};

use crate::{interop::StreamType};

const BUFFER_SIZE: usize = 1500;

static AUDIO_PEERS: OnceLock<Arc<PeerManager>> = OnceLock::new();
static FRAME_PEERS: OnceLock<Arc<PeerManager>> = OnceLock::new();
static LISTENER: OnceCell<TcpListener> = OnceCell::const_new();

async fn listener() -> &'static TcpListener {
    LISTENER.get_or_init(|| async {
        TcpListener::bind("0.0.0.0:5060").await.unwrap()
    }).await
}

// inject an instance of a peer manager for the server to manage
pub async fn run_signaling_server (
    peer_manager : Arc<PeerManager>,
    stream_type : StreamType
) -> io::Result<()> {
    
    let res = match stream_type {
        StreamType::Audio => AUDIO_PEERS.set(Arc::clone(&peer_manager)),
        StreamType::Video => FRAME_PEERS.set(Arc::clone(&peer_manager))
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
    response.put_slice(peer_manager.local_addr.to_string().as_bytes());

    let peers = peer_manager.get_peers().await;

    for addr in peers.iter() {
        if *addr != remote_addr {
            response.put_slice(b"\r\n");
            response.put_slice(addr.to_string().as_bytes());
        }
    }

    socket.write_all(&response).await?;

   Ok(()) 
}

pub async fn connect_to_signaling_server(
    server_addr: &str,
    peer_manager: Arc<PeerManager>,
    stream_type: StreamType
) -> io::Result<()> {
    let mut socket = TcpStream::connect(server_addr).await?;
    
    let str = peer_manager.local_addr.to_string() + "\r\n" + match stream_type {
        StreamType::Audio => "0",
        StreamType::Video => "1"
    };

    println!("{str}");

    socket.write_all(str.as_bytes()).await?;
    
    let mut buffer = [0u8; BUFFER_SIZE];
    let bytes_read = socket.read(&mut buffer).await?;
    
    if bytes_read == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "No response from server"));
    }

    let response = str::from_utf8(&buffer[..bytes_read])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    println!("Client List: ");

    for line in response.lines() {
        if let Ok(addr) = line.parse::<SocketAddr>() {
           
            let res = peer_manager.add_peer(addr).await;

            println!("{}, {}", addr.to_string(), res);
        }
    }

    Ok(())
}

pub struct PeerManager {
    addresses: Arc<Mutex<Vec<SocketAddr>>>,
    local_addr: SocketAddr,
}

impl PeerManager {
    pub fn new(local_addr: SocketAddr) -> Self {
        Self {
            addresses: Arc::new(Mutex::new(Vec::new())),
            local_addr,
        }
    }

    pub async fn add_peer(&self, addr: SocketAddr) -> bool {
        let mut addresses = self.addresses.lock().await;
        if !addresses.contains(&addr) && addr != self.local_addr {
            addresses.push(addr);
            true
        } else {
            false
        }
    }

    pub fn get_addresses_handle(&self) -> Arc<Mutex<Vec<SocketAddr>>> {
        Arc::clone(&self.addresses)
    }

    pub async fn get_peers(&self) -> Vec<SocketAddr> {
        self.addresses.lock().await.clone()
    }
}