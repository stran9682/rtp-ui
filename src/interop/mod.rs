use std::{io, net::SocketAddr, sync::{Arc, OnceLock}};

use tokio::{net::UdpSocket, runtime::Runtime, sync::mpsc};

use crate::session_management::{PeerManager, run_signaling_server};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

static FRAME_TX: OnceLock<mpsc::Sender<EncodedFrame>> = OnceLock::new();

pub struct EncodedFrame {
    pub data: Vec<u8>,
}

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Runtime::new().expect("Runtime creation failed. Loser")
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_send_frame(
    data: *const u8,
    len: usize
) {

    let tx = match FRAME_TX.get() {
        Some(tx) => tx,
        None => return,
    };

    let frame = unsafe {
        EncodedFrame {
            data: std::slice::from_raw_parts(data, len).to_vec(),
        }
    };

    let _ = tx.try_send(frame);
}

#[unsafe(no_mangle)]
pub extern "C" fn run_runtime_server () {
    let (tx, rx) = mpsc::channel::<EncodedFrame>(32);
    
    FRAME_TX.set(tx).ok();

    runtime().spawn(async {
        if let Err(e) = network_loop_server(rx).await {
            eprintln!("Something terrible happened. Not you though. You are amazing. Always: {}", e);
        }
    });
}

async fn network_loop_server (rx: mpsc::Receiver<EncodedFrame>) -> io::Result<()> {

    let local_addr_str = "127.0.0.1:8080".to_string();

    let local_addr: SocketAddr = local_addr_str  
        .parse()
        .expect("Invalid local address format");

    let socket = UdpSocket::bind(local_addr).await?;
    let socket = Arc::new(socket);

    let peer_manager = Arc::new(PeerManager::new(local_addr));

    let peer_manager_clone = Arc::clone(&peer_manager);

    runtime().spawn(async move {
        if let Err(e) = run_signaling_server(peer_manager_clone).await {
            eprintln!("Signaling server error: {}", e);
        }
    });

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

        for addr in peers.iter() {
            match socket.send_to(&frame.data, addr).await {
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