//
//  rtp_uiApp.swift
//  rtp-ui
//
//  Created by Sebastian Tran on 1/7/26.
//

import SwiftUI

@main
struct rtp_uiApp: App {
    
    @State var state = AppState(isPresented: true, isHost: false)
    
    var body: some Scene {
        WindowGroup {
            if state.isPresented {
                JoinView(state: $state)
            }
            else {
                ContentView()
            }
        }
    }
}

struct AppState {
    var isPresented: Bool
    var isHost: Bool
    var address: String = ""
}
