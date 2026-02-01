//
//  ContentView.swift
//  rtp-ui
//
//  Created by Sebastian Tran on 1/7/26.
//

import SwiftUI

struct ContentView: View {
    @State private var viewModel = ViewModel()
    
    @State public var peerVideoManager = PeerVideoManager()
    
    var body: some View {
        CameraView(image: $viewModel.currentFrame)
        
        Text("\(peerVideoManager.peers.count)")
    }
}
