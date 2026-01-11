use std::{net::SocketAddr, sync::Arc};
use bytes::{BufMut, BytesMut};
use tokio::{io::{self, AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream }, sync::Mutex};

const BUFFER_SIZE: usize = 1500;

pub async fn run_signaling_server (
    peer_manager : Arc<PeerManager>
) -> io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:5060").await?;

    loop {
        let (mut socket, client_addr) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
                continue;
            }
        }; 

        println!("Request from {}", client_addr.to_string());

        let peer_manager = Arc::clone(&peer_manager);

        tokio::spawn(async move {
            if let Err(e) = handle_signaling_client(&mut socket, peer_manager).await {
                eprintln!("Signaling error with {}: {}", client_addr, e);
            }
        });
    }
}

async fn handle_signaling_client (
    socket : &mut TcpStream, 
    peer_manager : Arc<PeerManager>

) -> io::Result<()> {
    let mut buffer = [0; BUFFER_SIZE];

    let bytes_read = socket.read(&mut buffer).await?;
    if bytes_read == 0 {
        return Ok(());
    }

    let remote_addr_str = std::str::from_utf8(&buffer[..bytes_read])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    
    let remote_addr: SocketAddr = remote_addr_str
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    println!("add address, {}", remote_addr_str);


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
) -> io::Result<()> {
    let mut socket = TcpStream::connect(server_addr).await?;
    
    socket.write_all(peer_manager.local_addr.to_string().as_bytes()).await?;
    
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