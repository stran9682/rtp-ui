//
//  CameraManager.swift
//  rtp-ui
//
//  Created by Sebastian Tran on 1/7/26.
//

import Foundation
import AVFoundation
import VideoToolbox
import RTPmacos

class CameraManager: NSObject {
    
    private var compressionSessionOut: VTCompressionSession?
    
    //  object that performs real-time capture and adds appropriate inputs and outputs
    private let captureSession = AVCaptureSession()
    
    //  describes the media input from a capture device to a capture session
    private var deviceInput : AVCaptureDeviceInput?
    
    //  object used to have access to video frames for processing
    private var videoOutput: AVCaptureVideoDataOutput? /// these lowkey aren't used but i'm keeping them here
    
    //  object that represents the hardware or virtual capture device
    //  that can provide one or more streams of media of a particular type
    private let systemPreferedCamera = AVCaptureDevice.default(for: .video)
    
    //  the queue on which the AVCaptureVideoDataOutputSampleBufferDelegate callbacks should be invoked.
    //  It is mandatory to use a serial dispatch queue to guarantee that video frames will be delivered in order
    private var sessionQueue = DispatchQueue(label: "video.preview.session")
    
    // Checks if the application has access to the camera
    private var isAuthorized: Bool {
        get async {
            let status = AVCaptureDevice.authorizationStatus(for: .video)
            
            // Determine if the user previously authorized camera access.
            var isAuthorized = status == .authorized
            
            // If the system hasn't determined the user's authorization status,
            // explicitly prompt them for approval.
            if status == .notDetermined {
                isAuthorized = await AVCaptureDevice.requestAccess(for: .video)
            }
            
            return isAuthorized
        }
    }
    
    private var addToPreviewStream: ((CGImage) -> Void)?
    
    //  manages the continuous stream of data provided by it
    //  through an AVCaptureVideoDataOutputSampleBufferDelegate object.
    lazy var previewStream: AsyncStream<CGImage> = {
        AsyncStream { continuation in
            addToPreviewStream = { cgImage in
                continuation.yield(cgImage)
            }
        }
    }()
    
    override init() {
        super.init()
        
        run_runtime_server(true, StreamType(1), nil, 0)    /// our rust code!
        //run_runtime_server(true, StreamType(0), nil, 0)
        
        Task {
            await configureSession()
            await startSession()
        }
    }
    
    //  responsible for initializing all our properties and defining the buffer delegate.
    private func configureSession() async {
        
        // Check user authorization,
        // if the selected camera is available,
        // and if can take the input through the AVCaptureDeviceInput object
        guard await isAuthorized,
              let systemPreferedCamera,
              let deviceInput = try? AVCaptureDeviceInput(device: systemPreferedCamera)
        else { return }
              
        // Start the configuration,
        // marking the beginning of changes to the running capture session’s configuration
        captureSession.beginConfiguration()
        captureSession.sessionPreset = .hd1280x720
        
        // At the end of the execution of the method commits the configuration to the running session
        defer {
            self.captureSession.commitConfiguration()
        }
        
        // Define the video output
        let videoOutput = AVCaptureVideoDataOutput()
        
        // set the Sample Buffer Delegate and the queue for invoking callbacks
        videoOutput.setSampleBufferDelegate(self, queue: sessionQueue)
        
        // Check if the input can be added to the capture session
        guard captureSession.canAddInput(deviceInput) else {
            print("Unable to add device input to capture session.")
            return
        }

        // Checking if the output can be added to the session
        guard captureSession.canAddOutput(videoOutput) else {
            print("Unable to add video output to capture session.")
            return
        }
        
        // Adds the input and the output to the AVCaptureSession
        captureSession.addInput(deviceInput)
        captureSession.addOutput(videoOutput)
        
        
        let videoEncoderSpecification = [kVTVideoEncoderSpecification_EnableLowLatencyRateControl: true as CFBoolean] as CFDictionary
        
        let err = VTCompressionSessionCreate(allocator: kCFAllocatorDefault,
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
        
        guard err == noErr, let compressionSession = compressionSessionOut else {
            print("VTCompressionSession creation failed")
            return
        }
        
        VTSessionSetProperty(compressionSession, key: kVTCompressionPropertyKey_RealTime, value: kCFBooleanTrue)
        VTSessionSetProperty(compressionSession, key: kVTCompressionPropertyKey_ProfileLevel, value: kVTProfileLevel_H264_Main_AutoLevel)
        VTSessionSetProperty(compressionSession, key: kVTCompressionPropertyKey_AllowFrameReordering, value: kCFBooleanFalse)
        VTSessionSetProperty(compressionSession, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: 30 as CFNumber)
        VTCompressionSessionPrepareToEncodeFrames(compressionSession)
    }
    
    //  will only be responsible for starting the camera session.
    private func startSession() async {
        guard await isAuthorized else { return }
        
        captureSession.startRunning()
    }
}

// MARK: WHAT THE FUCK!
private let outputCallback: VTCompressionOutputCallback = { refcon, sourceFrameRefCon, status, infoFlags, sampleBuffer in
    
    guard let refcon = refcon,
          status == noErr,
          let sampleBuffer = sampleBuffer else {
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
    
    var length = 0
    var dataPointer: UnsafeMutablePointer<Int8>?
    let status = CMBlockBufferGetDataPointer(dataBuffer, atOffset: 0, lengthAtOffsetOut: nil, totalLengthOut: &length, dataPointerOut: &dataPointer)
    
    rust_send_frame(dataPointer, UInt(length), StreamType(1))
}


extension CameraManager : AVCaptureVideoDataOutputSampleBufferDelegate { // honestly what the fuck
    
    func captureOutput(_ output: AVCaptureOutput,
                       didOutput sampleBuffer: CMSampleBuffer,
                       from connection: AVCaptureConnection) {
        
        guard let currentFrame = sampleBuffer.cgImage else { return }
        
        addToPreviewStream?(currentFrame)
        
        guard let session = compressionSessionOut,
              let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else {
            return
        }
        
        let presentationTimeStamp = CMSampleBufferGetPresentationTimeStamp(sampleBuffer)
        
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
