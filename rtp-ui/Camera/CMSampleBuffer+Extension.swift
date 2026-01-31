//
//  CMSampleBuffer+Extension.swift
//  rtp-ui
//
//  Created by Sebastian Tran on 1/7/26.
//

import Foundation
import CoreImage
import AVFoundation

extension CMSampleBuffer {
    var cgImage: CGImage? {
        let pixelBuffer: CVPixelBuffer? = CMSampleBufferGetImageBuffer(self)
        
        guard let imagePixelBuffer = pixelBuffer else { return nil }
        
        return CIImage(cvPixelBuffer: imagePixelBuffer).cgImage
    }
}
