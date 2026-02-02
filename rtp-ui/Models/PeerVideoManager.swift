//
//  PeerVideoManager.swift
//  rtp-ui
//
//  Created by Sebastian Tran on 1/31/26.
//

import Foundation
import RTPmacos

@Observable
class PeerVideoManager {
    
    var peers: [PeerVideoModel] = []
    
    init() {
        let refcon = Unmanaged.passUnretained(self).toOpaque()
        
        rust_send_video_callback(refcon, swift_receive_pps_sps)
    }
 
}

func swift_receive_pps_sps(
    _ context: UnsafeMutableRawPointer?,
    _ pps: UnsafePointer<UInt8>?,
    _ ppsLength: UInt,
    _ sps: UnsafePointer<UInt8>?,
    _ spsLength: UInt
) {
    guard let context = context else { return }
    
    let peerVideoManager = Unmanaged<PeerVideoManager>.fromOpaque(context).takeUnretainedValue()
    
    // copy the data - rust will drop the original
    let pps = Array(UnsafeBufferPointer(start: pps, count: Int(ppsLength)))
    let sps = Array(UnsafeBufferPointer(start: sps, count: Int(spsLength)))
    
    let newPeer = PeerVideoModel(pps: pps, sps: sps)
    
    DispatchQueue.main.async {
        peerVideoManager.peers.append(newPeer)
    }
}
