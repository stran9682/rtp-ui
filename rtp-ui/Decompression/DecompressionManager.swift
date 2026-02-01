//
//  VideoManager.swift
//  rtp-ui
//
//  Created by Sebastian Tran on 1/19/26.
//

import Foundation
import VideoToolbox

class DecompressionManager {
    var session: VTDecompressionSession?
        
    init (sps: UnsafePointer<UInt8>, spsLength: Int, pps: UnsafePointer<UInt8>, ppsLength: Int) {
       
        let paramSetPointers: [UnsafePointer<UInt8>] = [sps, pps]
        
        let parameterSetSizes: [Int] = [spsLength, ppsLength]
                
        var formatDescription: CMFormatDescription?
        
        CMVideoFormatDescriptionCreateFromH264ParameterSets(
            allocator: kCFAllocatorDefault,
            parameterSetCount: 2,
            parameterSetPointers: paramSetPointers,
            parameterSetSizes: parameterSetSizes,
            nalUnitHeaderLength: 4,
            formatDescriptionOut: &formatDescription
        )
        
        let decoderSpecification = [
            kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder: true as CFBoolean
        ] as CFDictionary
        
        let refcon = Unmanaged.passUnretained(self).toOpaque()
        var callbackRecord = VTDecompressionOutputCallbackRecord(
            decompressionOutputCallback: callback,
            decompressionOutputRefCon: refcon
        )
        
        if let formatDescription = formatDescription {
            VTDecompressionSessionCreate(
                allocator: kCFAllocatorDefault,
                formatDescription: formatDescription,
                decoderSpecification: decoderSpecification,
                imageBufferAttributes: nil,
                outputCallback: &callbackRecord,
                decompressionSessionOut: &session)
        }
    }
    
    func processImage(_ image: CVImageBuffer, time: CMTime, duration: CMTime) {
        
        var sampleBuffer: CMSampleBuffer?
        var sampleTiming = CMSampleTimingInfo(duration: duration, presentationTimeStamp: time, decodeTimeStamp: time)

        var formatDesc: CMFormatDescription? = nil
        CMVideoFormatDescriptionCreateForImageBuffer(
            allocator: kCFAllocatorDefault,
            imageBuffer: image,
            formatDescriptionOut: &formatDesc
        )
        
        guard let formatDescription = formatDesc else {
            fatalError("formatDescription")
        }

        let status = CMSampleBufferCreateReadyWithImageBuffer(
            allocator: kCFAllocatorDefault,
            imageBuffer: image,
            formatDescription: formatDescription,
            sampleTiming: &sampleTiming,
            sampleBufferOut: &sampleBuffer)
        
        if status != noErr {
            print("CMSampleBufferCreateReadyWithImageBuffer failure \(status)")
        }
//        if let sb = sampleBuffer {
//            handler?(sb)
//        }
    }
}

var callback: VTDecompressionOutputCallback = { refcon, sourceFrameRefCon, status, infoFlags, imageBuffer, time, duration in
    guard let refcon = refcon,
          status == noErr,
          let imageBuffer = imageBuffer
    else {
        print("VTDecompressionOutputCallback \(status)")
        return
    }
    
    let decoder = Unmanaged<DecompressionManager>.fromOpaque(refcon).takeUnretainedValue()
    decoder.processImage(imageBuffer, time: time, duration: duration)
}
