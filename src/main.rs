use bytes::{BufMut, BytesMut};
use tokio::{io::{AsyncReadExt, AsyncWriteExt }, net::{TcpListener, TcpStream, UdpSocket}, sync::Mutex};
use std::{env, io, net::{ SocketAddr}, sync::Arc};

#[tokio::main]
async fn main() -> io::Result<()> {

    let addresses :  Arc<Mutex<Vec<SocketAddr>>> = Arc::new(Mutex::new(Vec::new()));
    let send_to = Arc::clone(&addresses);
    let members= Arc::clone(&addresses);
   
    let args : Vec<String> = env::args().skip(1).collect();

    // second arg is where your RTP packets will go
    let local_addr = if args.len() == 2 {
        args[1].clone()
    } else {
        "0.0.0.0:8080".to_string()
    };

    let sock = UdpSocket::bind(local_addr.parse::<SocketAddr>().unwrap()).await?;
    let r = Arc::new(sock);
    let s = r.clone();

    // first arg is SIP socket. 
    if let Some(arg) = args.first() {
        let mut sip_sock =  TcpStream::connect(arg).await?;

        sip_sock.write_all(local_addr.as_bytes()).await?;

        let mut buffer = [0; 1024];

        let bytes_read = sip_sock.read(&mut buffer).await?;

        let response = str::from_utf8(&buffer[..bytes_read]).unwrap();


        let mut addresses = addresses.lock().await;

        for line in response.lines() {
            // println!("{line}");

            let remote_addr: SocketAddr = line
                .parse()
                .unwrap();

            addresses.push(remote_addr);
        }
    }
    else {
        tokio::spawn( async move {
            let mut buffer = [0; 1024];

            let listener = TcpListener::bind("0.0.0.0:5060").await.unwrap();

            loop {
                let (mut socket, _) = listener.accept().await.unwrap();

                let bytes_read = socket.read(&mut buffer).await.unwrap();

                let remote_addr: SocketAddr = str::from_utf8(&buffer[..bytes_read]).unwrap()
                    .parse()
                    .unwrap();

                let mut addresses = addresses.lock().await;

                if addresses.contains(&remote_addr) {
                    continue; 
                }  

                let mut buf = BytesMut::with_capacity(64);

                buf.put_slice((local_addr.to_string()).as_bytes());

                for address in addresses.iter() {
                    buf.put_slice(("\r\n".to_string() + &address.to_string()).as_bytes());
                }

                socket.write_all(&buf).await.unwrap();

                addresses.push(remote_addr)  
            }
        });
    }

    // sending
    tokio::spawn(async move {
        loop {
            let mut guess = String::new();

            // eventually replace this with video and audio stream
            io::stdin()
                .read_line(&mut guess)
                .expect("Failed to read line");

            let send_to = send_to.lock().await;

            for addr in send_to.iter() {
                s.send_to(&guess.as_bytes(), &addr).await.unwrap();
            }
        }
    });

    // receiving
    let mut buf = [0; 1024];
    loop {
        let (n , addr) = r.recv_from(&mut buf).await?;

        // let mut addresses = members.lock().await;

        // if !addresses.contains(&addr) {
        //     addresses.push(addr);
        // } 
        
        print!("{}: {}", addr.to_string(), str::from_utf8(&buf[..n]).unwrap());
    }
}