use std::{ env, io, sync::Arc};
use bytes::{BufMut, Bytes};
use tokio::{ net::{UdpSocket}};
use rtp::{interop::StreamType, packets::rtp::RTPHeader, session_management::{peer_manager::PeerManager, signaling_server::{ PEER_SPECIFICATIONS, PeerSpecifications, connect_to_signaling_server, run_signaling_server}}};

#[tokio::main]
async fn main() -> io::Result<()> {

    let args : Vec<String> = env::args().skip(1).collect(); 

    // Dynamically assign port
    let local_addr_str = "127.0.0.1:0";

    let socket = UdpSocket::bind(local_addr_str).await?;
    let socket = Arc::new(socket);

    let peer_manager = Arc::new(PeerManager::new(socket.local_addr()?));

    let sps= Bytes::copy_from_slice(&[0]);
    let pps = Bytes::copy_from_slice(&[0]);

    let _ = PEER_SPECIFICATIONS.set(PeerSpecifications::new(pps, sps));

    // get peers
    if let Some(server_addr) = args.first() {
        println!("Connecting:");

        if let Err(e) = connect_to_signaling_server(Some(server_addr.to_string()), Arc::clone(&peer_manager), StreamType::Video).await {
            eprintln!("server error getting addresses: {}", e);
        };

    // or be responsible for distributing them (rendevouz)
    } 
    
    println!("Starting Signaling Server:");

    let peer_manager_clone = Arc::clone(&peer_manager);
    tokio::spawn(async move {
        if let Err(e) = run_signaling_server(peer_manager_clone, StreamType::Video).await {
            eprintln!("Signaling server error: {}", e);
        }
    });
    

    let sender_socket = Arc::clone(&socket);
    let sender_peers = Arc::clone(&peer_manager);
    tokio::spawn(async move {
        rtp_sender(sender_socket, sender_peers).await;
    });

    rtp_receiver(socket, peer_manager).await
}

async fn rtp_sender(
    socket: Arc<UdpSocket>,
    peer_manager: Arc<PeerManager>
) {    
    loop {
        let mut input = String::new();
        
        if let Err(e) = io::stdin().read_line(&mut input) {
            eprintln!("Failed to read input: {}", e);
            continue;
        }

        let peers = peer_manager.get_peers();
        
        if peers.is_empty() {
            continue;
        }

        for addr in peers.iter() {
            match socket.send_to(&input.as_bytes(), addr).await {
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

    let mut buffer = [0u8; 1500];
    
    loop {
        let (bytes_read, addr) = socket.recv_from(&mut buffer).await?;

        if peer_manager.add_peer(addr) {
            println!("new peer from: {}", addr);
        }

        let mut bytes = bytes::BytesMut::new();
        bytes.put_slice(&buffer[..bytes_read]);

        let rtp = RTPHeader::deserialize(&mut bytes);

        println!("{}", rtp.timestamp);
        println!("{}", rtp.marker);

        println!("{:08b}{:08b}", buffer[0], buffer[1]);
    }
}