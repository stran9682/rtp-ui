use std::{ env, io, net::SocketAddr, sync::Arc};
use tokio::{ net::{UdpSocket}};
use rtp::session_management::{PeerManager, connect_to_signaling_server, run_signaling_server};

#[tokio::main]
async fn main() -> io::Result<()> {

    let args : Vec<String> = env::args().skip(1).collect(); 

    // second arg is where your rtp packets are received, otherwise default to 8080
    let local_addr_str = if args.len() == 2 {
        args[1].clone()
    } else {
        "127.0.0.1:8080".to_string()
    };

    let local_addr: SocketAddr = local_addr_str
        .parse()
        .expect("Invalid local address format");

    let socket = UdpSocket::bind(local_addr).await?;
    let socket = Arc::new(socket);

    let peer_manager = Arc::new(PeerManager::new(local_addr));

    // get peers
    if let Some(server_addr) = args.first() {
        println!("Connecting:");

        connect_to_signaling_server(server_addr, Arc::clone(&peer_manager)).await?;

    // or be responsible for distributing them (rendevouz)
    } else {
        println!("Starting Signaling Server:");

        let peer_manager_clone = Arc::clone(&peer_manager);
        tokio::spawn(async move {
            if let Err(e) = run_signaling_server(peer_manager_clone).await {
                eprintln!("Signaling server error: {}", e);
            }
        });
    }

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

        let peers = peer_manager.get_peers().await;
        
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

    let mut buffer = [0u8; 1024];
    
    loop {
        let (bytes_read, addr) = socket.recv_from(&mut buffer).await?;

        if peer_manager.add_peer(addr).await {
            println!("new peer from: {}", addr);
        }

        print!("{}: {}", addr.to_string(), str::from_utf8(&buffer[..bytes_read]).unwrap());
    }
}