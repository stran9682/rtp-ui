//
//  CompressionManager.swift
//  rtp-ui
//
//  Created by Sebastian Tran on 1/31/26.
//

import Foundation
import VideoToolbox
import RTPmacos

class CompressionManager {
    
    private var compressionSessionOut: VTCompressionSession?
    
    init () {
        let videoEncoderSpecification = [kVTVideoEncoderSpecification_EnableLowLatencyRateControl: true as CFBoolean] as CFDictionary
        
        VTCompressionSessionCreate(allocator: kCFAllocatorDefault,
                                         width: Int32(1280),
                                         height: Int32(720),
                                         // MARK: Copied from above ^ in session create
                                         codecType: kCMVideoCodecType_H264,
                                         encoderSpecification: videoEncoderSpecification,
                                         imageBufferAttributes: nil,
                                         compressedDataAllocator: nil,
                                         outputCallback: outputCallback,
                                         refcon: Unmanaged.passUnretained(self).toOpaque(), // WHAT DOES THIS DO?
                                         compressionSessionOut: &compressionSessionOut)
        
    }
    
    public func compressFrame(pixelBuffer : CVImageBuffer, presentationTimeStamp: CMTime) {
        guard let session = compressionSessionOut else {
            return
        }
        
        let status = VTCompressionSessionEncodeFrame(
            session,
            imageBuffer: pixelBuffer,
            presentationTimeStamp: presentationTimeStamp,
            duration: .invalid,
            frameProperties: nil,
            sourceFrameRefcon: nil,
            infoFlagsOut: nil
        )
        
        if status != noErr {
            print("Encoding failed: \(status)")
        }

    }
}

private let outputCallback: VTCompressionOutputCallback = { refcon, sourceFrameRefCon, status, infoFlags, sampleBuffer in
    
    guard let refcon = refcon,
          status == noErr,
          let sampleBuffer = sampleBuffer
    else {
        print("H264Coder outputCallback sampleBuffer NULL or status: \(status)")
        return
    }
    
    if (!CMSampleBufferDataIsReady(sampleBuffer))
    {
        print("didCompressH264 data is not ready...");
        return;
    }
    
    guard let dataBuffer = CMSampleBufferGetDataBuffer(sampleBuffer) else {
        print("Failed to convert buffer")
        return
    }
    
    // MARK: Transmitting SPS and PPS data.
    // https://stackoverflow.com/questions/28396622/extracting-h264-from-cmblockbuffer
    
    guard let attachmentsArray:CFArray = CMSampleBufferGetSampleAttachmentsArray(
        sampleBuffer,
        createIfNecessary: false
    ) else { return }
    
    // this becomes a really redundant check. Only works once every time h264 parameters change!
    if (CFArrayGetCount(attachmentsArray) > 0) {
    
        let cfDict = CFArrayGetValueAtIndex(attachmentsArray, 0)
        let dictRef: CFDictionary = unsafeBitCast(cfDict, to: CFDictionary.self)

        let value = CFDictionaryGetValue(dictRef, unsafeBitCast(kCMSampleAttachmentKey_NotSync, to: UnsafeRawPointer.self))
        
        if(value == nil) {
            var description: CMFormatDescription = CMSampleBufferGetFormatDescription(sampleBuffer)!
            
                        
            // First, get SPS
            var sparamSetCount: size_t = 0
            var sparamSetSize: size_t = 0
            var sparameterSetPointer: UnsafePointer<UInt8>?
            var s_statusCode: OSStatus = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                description,
                parameterSetIndex: 0,
                parameterSetPointerOut: &sparameterSetPointer,
                parameterSetSizeOut: &sparamSetSize,
                parameterSetCountOut: &sparamSetCount,
                nalUnitHeaderLengthOut: nil)
        
            // Then, get PPS
            var pparamSetCount: size_t = 0
            var pparamSetSize: size_t = 0
            var pparameterSetPointer: UnsafePointer<UInt8>?
            var p_statusCode: OSStatus = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                description,
                parameterSetIndex: 1,
                parameterSetPointerOut: &pparameterSetPointer,
                parameterSetSizeOut: &pparamSetSize,
                parameterSetCountOut: &pparamSetCount,
                nalUnitHeaderLengthOut: nil)
            
            if (p_statusCode == noErr && s_statusCode == noErr) {
                rust_send_h264_config(pparameterSetPointer, UInt(pparamSetSize), sparameterSetPointer, UInt(sparamSetSize))
            }
            
            let spsArray = Array(UnsafeBufferPointer(start: pparameterSetPointer, count: pparamSetSize))
            print("let sps: [UInt8] = \(spsArray)")
        }
    }
    
    // MARK: Pointers to data
    
    // the h.264 data, get pointer to cmblockbuffer
    var length = 0
    var dataPointer: UnsafeMutablePointer<Int8>?
    let status = CMBlockBufferGetDataPointer(dataBuffer, atOffset: 0, lengthAtOffsetOut: nil, totalLengthOut: &length, dataPointerOut: &dataPointer)
    
    guard status == noErr, let dataPointer = dataPointer else { return }
    
    // now, the data to the holding object (sample buffer)
    let unmanagedBuffer = Unmanaged.passRetained(sampleBuffer)  // increments the counter
    let context = unmanagedBuffer.toOpaque()                    // get a pointer to pass to C
    
    rust_send_frame(dataPointer, UInt(length), context, swift_release_frame_buffer)
}

func swift_release_frame_buffer(_ context: UnsafeMutableRawPointer?) {
    guard let context = context else { return }
    
    // Release the manual retain
    let _ = Unmanaged<CMSampleBuffer>.fromOpaque(context).takeRetainedValue()
}
