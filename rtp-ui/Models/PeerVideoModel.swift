//
//  PeerVideoModel.swift
//  rtp-ui
//
//  Created by Sebastian Tran on 1/31/26.
//

import Foundation

class PeerVideoModel {
    
    private var videoManager: DecompressionManager? = nil
    private var sps : [UInt8] = []
    private var pps : [UInt8] = []
    
    init(pps: [UInt8], sps: [UInt8]) {
        self.pps = pps
        self.sps = sps
        
        videoManager = DecompressionManager(
            sps: self.sps,
            spsLength: sps.count,
            pps: self.pps,
            ppsLength: pps.count
        )
    }
}
