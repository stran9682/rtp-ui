use std::{collections::VecDeque, net::SocketAddr, u128::MAX};
use bytes::Bytes;
use dashmap::DashMap;

pub struct PlayoutBufferNode {
    pub arrival_time : u128,
    pub rtp_timestamp : u32,
    pub playout_time : u128, 
    pub coded_data : Vec<Fragment>
}

pub struct Fragment {
    pub sequence_num : u16,
    pub data : Bytes
}

pub struct Peer {
    window : VecDeque<u128>,
    min_window : u128,
    playout_buffer : Vec<PlayoutBufferNode>
}

impl Peer {
    pub fn set_and_get_min_window (&mut self, difference : u128) -> u128{

        self.window.push_front(difference);

        if self.window.len() > 10 {
            self.window.pop_back();
        }

        if let Some(&min_val) = self.window.iter().min() {
            self.min_window = min_val;
        }

        self.min_window = match self.window.iter().min() {
            Some(val) => *val,
            None => difference
        };

        return self.min_window
    }
}

pub struct PeerManager {
    peers: DashMap<SocketAddr, Peer>,
    pub local_addr: SocketAddr,
}

impl PeerManager {
    pub fn new(local_addr: SocketAddr) -> Self {
        Self {
            peers: DashMap::new(),
            local_addr,
        }
    }

    pub fn add_peer(&self, addr: SocketAddr) -> bool {
        let peers = &self.peers;
      
        if  !peers.contains_key(&addr) && addr != self.local_addr {
            peers.insert(addr, Peer{
                window: VecDeque::new(),
                min_window: MAX,
                playout_buffer: Vec::new()
            });

            true
        } else {
            false
        }
    }

    pub fn peer_get_min_window(&self, addr: SocketAddr, difference: u128) -> u128 {
        let peers = &self.peers;

        if let Some(mut found_peer) = peers.get_mut(&addr) {
            found_peer.set_and_get_min_window(difference)
        } else {
            // peers.insert(addr, Peer{
            //     window: VecDeque::new(),
            //     min_window: difference,
            //     playout_buffer: Vec::new()
            // });

            difference
        }
    }

    pub fn add_playout_node_to_peer(&self, addr: SocketAddr, mut playout_buffer_node : PlayoutBufferNode, fragment: Fragment){
        let peers = &self.peers;

        let Some(mut peer) = peers.get_mut(&addr) else {
            return
        };

        let timestamp = playout_buffer_node.rtp_timestamp;

        match peer.playout_buffer.binary_search_by_key(&timestamp, |node| node.rtp_timestamp) {
            Ok(index) => {

                let coded_data = &mut peer.playout_buffer[index].coded_data;

                match coded_data.binary_search_by_key(&fragment.sequence_num, |frag| frag.sequence_num) {
                    _ => {
                        coded_data.insert(index, fragment);
                    }
                }
            }
            Err(index) => {
                playout_buffer_node.coded_data.push(fragment);
                peer.playout_buffer.insert(index, playout_buffer_node);
            }
        }
    }

    pub fn get_peers(&self) -> Vec<SocketAddr> {
        self.peers.iter().map(|entry| entry.key().clone()).collect()
    }

    pub fn pop_node(&self, addr: SocketAddr) -> Option<PlayoutBufferNode> {
        let mut peer = self.peers.get_mut(&addr)?;

        let Some(node) = peer.playout_buffer.pop() else {
            return  None;
        };

        Some(node)
    }
}