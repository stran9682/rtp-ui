//
//  JoinView.swift
//  rtp-ui
//
//  Created by Sebastian Tran on 1/10/26.
//

import SwiftUI
import RTPmacos

struct JoinView: View {
    
    @Binding var state: Bool
    @State private var address = ""
    
    var body: some View {
        
        VStack {
            Button(action: {
                state = false
            }, label: {
                Text("Start Session")
            })
                .padding()
            
            TextField("Enter SIP address", text: $address)
                .textFieldStyle(.roundedBorder)
                .frame(maxWidth: 200)

            Button(action: {
                state = false
                address.withCString { pointer in
                    rust_set_signalling_addr(pointer, UInt(strlen(pointer)))
                }
            }, label: {
                Text("Submit")
            })

        }
        .frame(minWidth: 500, minHeight: 300)
    }
}
