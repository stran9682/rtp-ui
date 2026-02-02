//
//  ViewModel.swift
//  rtp-ui
//
//  Created by Sebastian Tran on 1/7/26.
//

import Foundation
import CoreImage
import Observation

@Observable
class ViewModel {
    var currentFrame: CGImage?
    
    private let cameraManager = CameraManager()
    
    init() {
        Task {
            await handleCameraPreviews()
        }
    }
    
    func handleCameraPreviews() async {
        for await image in cameraManager.previewStream {
            Task {
                @MainActor in currentFrame = image
            }
        }
    }
}
