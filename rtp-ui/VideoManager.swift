//
//  VideoManager.swift
//  rtp-ui
//
//  Created by Sebastian Tran on 1/19/26.
//

import Foundation
import VideoToolbox

class VideoManager {
    var session: VTDecompressionSession?
    
    init () {
        
        let sps: [UInt8] = [39, 77, 0, 31, 137, 138, 48, 10, 0, 183, 77, 64, 128, 128, 129, 225, 0, 132, 208]
        let pps: [UInt8] = [40, 238, 60, 128]
       
        let paramSetPointers: [UnsafePointer<UInt8>] = [UnsafePointer(sps), UnsafePointer(pps)]
        
        let parameterSetSizes: [Int] = [sps.count, pps.count]
        
        let decoderSpecification = [
            kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder: true as CFBoolean
        ] as CFDictionary
        
        
        var formatDescription: CMFormatDescription?
        
        CMVideoFormatDescriptionCreateFromH264ParameterSets(allocator: kCFAllocatorDefault, parameterSetCount: 2, parameterSetPointers: paramSetPointers, parameterSetSizes: parameterSetSizes, nalUnitHeaderLength: 4, formatDescriptionOut: &formatDescription)
    }
}
