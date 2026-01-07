//
//  CIImage+Extension.swift
//  rtp-ui
//
//  Created by Sebastian Tran on 1/7/26.
//

import Foundation
import CoreImage

extension CIImage {
    
    var cgImage: CGImage? {
        let ciContext = CIContext()
        
        guard let cgImage = ciContext.createCGImage(self, from: extent) else {
            return nil
        }
        
        return cgImage
    }
    
    
}
