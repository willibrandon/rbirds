// Visible-window sampling only; this cannot establish physical display scanout.
// Build with: swiftc -O -parse-as-library tools/presentation-capture.swift -o target/presentation-capture
// Run with: target/presentation-capture rbirds-perf-TITLE target/presentation.json [seconds]
import Foundation
import ScreenCaptureKit
import CoreMedia
import AppKit
import Darwin

final class Recorder: NSObject, SCStreamOutput {
    var frames: [[String: Any]] = []
    var lastHash: UInt64? = nil
    func stream(_ stream: SCStream, didOutputSampleBuffer sample: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, sample.isValid,
            let attachments = CMSampleBufferGetSampleAttachmentsArray(sample, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
            let info = attachments.first, let status = info[.status] as? Int,
            status == SCFrameStatus.complete.rawValue,
            let pixels = CMSampleBufferGetImageBuffer(sample) else { return }
        CVPixelBufferLockBaseAddress(pixels, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixels, .readOnly) }
        guard let address = CVPixelBufferGetBaseAddress(pixels) else { return }
        let width = CVPixelBufferGetWidth(pixels), height = CVPixelBufferGetHeight(pixels)
        let stride = CVPixelBufferGetBytesPerRow(pixels)
        var hash: UInt64 = 1469598103934665603
        // Sample only the content area; title-bar changes do not count as animation.
        for y in Swift.stride(from: 40, to: height - 4, by: 8) {
            let row = address.advanced(by: y * stride).assumingMemoryBound(to: UInt32.self)
            for x in Swift.stride(from: 4, to: width - 4, by: 8) {
                hash = (hash ^ UInt64(row[x])) &* 1099511628211
            }
        }
        frames.append(["pts": CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sample)), "changed": lastHash != hash, "hash": String(hash)])
        lastHash = hash
    }
}
@main struct Capture {
    static func main() async throws {
        NSApplication.shared.setActivationPolicy(.prohibited)
        guard (3...4).contains(CommandLine.arguments.count) else {
            fatalError("Usage: presentation-capture rbirds-perf-TITLE output.json [seconds]")
        }
        let seconds = CommandLine.arguments.count == 4 ? Double(CommandLine.arguments[3]) ?? 0 : 12
        guard seconds.isFinite, seconds >= 1, seconds <= 60 else {
            fatalError("Duration must be between 1 and 60 seconds")
        }
        let title = CommandLine.arguments[1], output = CommandLine.arguments[2]
        guard title.hasPrefix("rbirds-perf-") else { fatalError("Only test windows") }
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        guard let window = content.windows.first(where: { $0.title == title }) else { fatalError("No matching test window") }
        let filter = SCContentFilter(desktopIndependentWindow: window)
        let configuration = SCStreamConfiguration()
        configuration.width = Int(window.frame.width)
        configuration.height = Int(window.frame.height)
        configuration.pixelFormat = kCVPixelFormatType_32BGRA
        configuration.minimumFrameInterval = CMTime(value: 1, timescale: 120)
        configuration.queueDepth = 5
        configuration.showsCursor = false
        let recorder = Recorder()
        let queue = DispatchQueue(label: "rbirds.capture")
        let stream = SCStream(filter: filter, configuration: configuration, delegate: nil)
        try stream.addStreamOutput(recorder, type: .screen, sampleHandlerQueue: queue)
        var started = timespec(); clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &started)
        try await stream.startCapture()
        try await Task.sleep(nanoseconds: UInt64(seconds * 1e9))
        try? await stream.stopCapture()
        queue.sync {}
        var finished = timespec(); clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &finished)
        let cpu = Double(finished.tv_sec - started.tv_sec) + Double(finished.tv_nsec - started.tv_nsec) / 1e9
        let report: [String: Any] = ["scope": "ScreenCaptureKit samples of visible test window, not physical scanout", "window": title, "capture_cpu_seconds": cpu, "requested_seconds": seconds, "size": [configuration.width, configuration.height], "requested_capture_hz": 120, "frames": recorder.frames]
        let data = try JSONSerialization.data(withJSONObject: report, options: [.sortedKeys])
        try data.write(to: URL(fileURLWithPath: output))
        print(title, recorder.frames.count, "capture CPU seconds", cpu)
    }
}
