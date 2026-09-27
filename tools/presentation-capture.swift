// Visible-window sampling only; this cannot establish physical display scanout.
// Build with: swiftc -O -parse-as-library tools/presentation-capture.swift -o target/presentation-capture
// Run with: target/presentation-capture rbirds-perf-TITLE target/presentation.json [seconds] [RRGGBB ...]
import Foundation
import ScreenCaptureKit
import CoreMedia
import AppKit
import Darwin

func frontContentWindow(_ windows: [[String: Any]], content: CGRect) -> [String: Any]? {
    windows.first {
        guard ($0[kCGWindowLayer as String] as? Int) == 0,
              ($0[kCGWindowAlpha as String] as? Double ?? 0) > 0,
              let bounds = $0[kCGWindowBounds as String] as? [String: Any],
              let frame = CGRect(dictionaryRepresentation: bounds as CFDictionary) else { return false }
        return frame.intersects(content)
    }
}

func checkFocusWindows() throws {
    let content = CGRect(x: 64, y: 100, width: 702, height: 537)
    func window(_ id: UInt32, _ frame: CGRect, layer: Int = 0, alpha: Double = 1) -> [String: Any] {
        [kCGWindowNumber as String: id, kCGWindowBounds as String: frame.dictionaryRepresentation,
         kCGWindowLayer as String: layer, kCGWindowAlpha as String: alpha]
    }
    let buttons = window(1, CGRect(x: 66, y: 66, width: 66, height: 20))
    let overlay = window(2, content, layer: 8)
    let target = window(3, CGRect(x: 60, y: 60, width: 710, height: 581))
    let covered = window(4, CGRect(x: 80, y: 120, width: 50, height: 50))
    let invisible = window(5, content, alpha: 0)
    let cases: [([[String: Any]], UInt32?)] = [
        ([buttons, overlay, invisible, target], 3),
        ([buttons, covered, target], 4),
        ([buttons, invisible], nil)
    ]
    for (windows, expected) in cases {
        guard frontContentWindow(windows, content: content)?[kCGWindowNumber as String] as? UInt32 == expected else {
            throw CaptureError.invalidArguments("Content-window focus comparison failed")
        }
    }
}

// Retain the content pixels, not the capture surface, which belongs to the
// stream's buffer pool. Row comparisons ignore title bars, borders and padding.
struct ContentChanges {
    var previous = Data()
    var previousWidth = 0
    var previousHeight = 0

    mutating func changed(_ address: UnsafeRawPointer, width: Int, height: Int, stride: Int) -> Bool {
        let rowBytes = max(0, width - 8) * 4
        let rows = max(0, height - 44)
        var changed = width != previousWidth || height != previousHeight
        if changed {
            previous = Data(count: rows * rowBytes)
            previousWidth = width
            previousHeight = height
        }
        previous.withUnsafeMutableBytes { destination in
            guard let stored = destination.baseAddress, rowBytes > 0 else { return }
            for row in 0..<rows {
                let source = address.advanced(by: (row + 40) * stride + 4 * 4)
                let target = stored.advanced(by: row * rowBytes)
                if memcmp(source, target, rowBytes) != 0 {
                    changed = true
                    memcpy(target, source, rowBytes)
                }
            }
        }
        return changed
    }
}

func checkContentChanges() throws {
    var detector = ContentChanges()
    var bytes = Data(count: 160 * 60)
    func changed(_ data: Data, width: Int = 32, stride: Int = 160) -> Bool {
        data.withUnsafeBytes { detector.changed($0.baseAddress!, width: width, height: 60, stride: stride) }
    }
    func expect(_ condition: Bool, _ detail: String) throws {
        if !condition { throw CaptureError.invalidArguments("pixel comparison check failed: " + detail) }
    }
    try expect(changed(bytes), "first frame")
    try expect(!changed(bytes), "identical frame")
    bytes[2 * 160 + 8 * 4] = 7 // Title bar.
    bytes[50 * 160 + 32 * 4] = 9 // Row padding.
    try expect(!changed(bytes), "ignore title bar and padding")
    bytes[51 * 160 + 9 * 4] = 17 // Between the old eight-pixel sample locations.
    try expect(changed(bytes), "single content pixel outside sampling grid")
    try expect(!changed(bytes), "unchanged small detail")
    try expect(changed(bytes, width: 33), "resize")
    var padded = Data(count: 192 * 60)
    for row in 0..<60 {
        padded.replaceSubrange(row * 192..<row * 192 + 33 * 4,
                               with: bytes[row * 160..<row * 160 + 33 * 4])
    }
    try expect(!changed(padded, width: 33, stride: 192), "same pixels with a different row stride")
    print("Content pixel comparisons passed")
}

final class Recorder: NSObject, SCStreamOutput {
    var frames: [[String: Any]] = []
    var lastHash: UInt64? = nil
    var watchedColours: [UInt32] = []
    var contentChanges = ContentChanges()
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
        var colours: [UInt32: Int] = [:]
        var sampled = 0
        var translucent = 0
        // Colour diagnostics stay sampled; animation detection compares every
        // content pixel so small glyph changes cannot fall between samples.
        for y in Swift.stride(from: 40, to: height - 4, by: 8) {
            let row = address.advanced(by: y * stride).assumingMemoryBound(to: UInt32.self)
            for x in Swift.stride(from: 4, to: width - 4, by: 8) {
                let pixel = row[x]
                hash = (hash ^ UInt64(pixel)) &* 1099511628211
                colours[pixel & 0x00ff_ffff, default: 0] += 1
                if pixel >> 24 != 255 { translucent += 1 }
                sampled += 1
            }
        }
        let changed = contentChanges.changed(address, width: width, height: height, stride: stride)
        var frame: [String: Any] = ["pts": CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sample)), "changed": changed, "sampled_changed": lastHash != hash, "hash": String(hash)]
        if let dominant = colours.max(by: { a, b in
            a.value == b.value ? a.key < b.key : a.value < b.value
        }) {
            frame["dominant_rgb"] = [(dominant.key >> 16) & 255, (dominant.key >> 8) & 255, dominant.key & 255]
            frame["dominant_fraction"] = Double(dominant.value) / Double(sampled)
        }
        frame["sampled_pixels"] = sampled
        frame["translucent_samples"] = translucent
        if !watchedColours.isEmpty {
            frame["watched_colours"] = watchedColours.map { rgb -> [String: Any] in
                return ["rgb": [(rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255],
                        "samples": colours[rgb, default: 0]]
            }
        }
        frames.append(frame)
        lastHash = hash
    }
}
enum CaptureError: Error {
    case invalidArguments(String)
    case windowNotFound(String)
}

@main struct Capture {
    static func main() async {
        do {
            if CommandLine.arguments.count == 2 && CommandLine.arguments[1] == "--self-test" {
                try checkContentChanges()
                try checkFocusWindows()
                print("Content-window focus comparisons passed")
                return
            }
            try await capture()
        } catch {
            FileHandle.standardError.write(Data("presentation-capture: \(error)\n".utf8))
            exit(1)
        }
    }

    @MainActor static func capture() async throws {
        NSApplication.shared.setActivationPolicy(.prohibited)
        guard (3...12).contains(CommandLine.arguments.count) else {
            throw CaptureError.invalidArguments("Usage: presentation-capture rbirds-perf-TITLE output.json [seconds] [RRGGBB ...] (up to 8 colours)")
        }
        let seconds = CommandLine.arguments.count >= 4 ? Double(CommandLine.arguments[3]) ?? 0 : 12
        guard seconds.isFinite, seconds >= 1, seconds <= 60 else {
            throw CaptureError.invalidArguments("Duration must be between 1 and 60 seconds")
        }
        let title = CommandLine.arguments[1], output = CommandLine.arguments[2]
        guard title.hasPrefix("rbirds-perf-") else {
            throw CaptureError.invalidArguments("Only titles starting with rbirds-perf- are accepted")
        }
        let watchedColours = try CommandLine.arguments.dropFirst(4).map { value -> UInt32 in
            guard value.utf8.count == 6,
                value.utf8.allSatisfy({ (48...57).contains($0) || (65...70).contains($0) || (97...102).contains($0) }),
                let rgb = UInt32(value, radix: 16) else {
                throw CaptureError.invalidArguments("Watched colours must be six hexadecimal digits")
            }
            return rgb
        }
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        let matches = content.windows.filter { $0.title == title }
        guard matches.count == 1, let window = matches.first else {
            throw CaptureError.windowNotFound("Expected one visible window titled \(title), found \(matches.count)")
        }
        let filter = SCContentFilter(desktopIndependentWindow: window)
        let configuration = SCStreamConfiguration()
        configuration.width = Int(window.frame.width)
        configuration.height = Int(window.frame.height)
        configuration.pixelFormat = kCVPixelFormatType_32BGRA
        configuration.minimumFrameInterval = CMTime(value: 1, timescale: 120)
        configuration.queueDepth = 5
        configuration.showsCursor = false
        let recorder = Recorder()
        recorder.watchedColours = watchedColours
        let queue = DispatchQueue(label: "rbirds.capture")
        let stream = SCStream(filter: filter, configuration: configuration, delegate: nil)
        try stream.addStreamOutput(recorder, type: .screen, sampleHandlerQueue: queue)
        var started = timespec(); clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &started)
        try await stream.startCapture()
        let focusStart = DispatchTime.now().uptimeNanoseconds
        let focusEnd = focusStart + UInt64(seconds * 1e9)
        let contentFrame = CGRect(x: window.frame.minX + 4, y: window.frame.minY + 40,
                                  width: max(0, window.frame.width - 8), height: max(0, window.frame.height - 44))
        var focus: [[String: Any]] = []
        // A desktop-independent capture can keep receiving an occluded or
        // unfocused window. Record focus separately rather than treating
        // onScreenWindowsOnly as proof that the test window was in front.
        while true {
            let now = DispatchTime.now().uptimeNanoseconds
            let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
            // macOS can give a window's title-bar buttons their own normal
            // window above it. Only windows overlapping captured content count.
            let top = frontContentWindow(windows, content: contentFrame)
            var observation: [String: Any] = [
                "elapsed_seconds": Double(now - focusStart) / 1e9,
                "owner_active": window.owningApplication.map { $0.processID == NSWorkspace.shared.frontmostApplication?.processIdentifier } ?? false,
                "top_normal_window": (top?[kCGWindowNumber as String] as? UInt32) == window.windowID
            ]
            if let top {
                observation["top_window_id"] = top[kCGWindowNumber as String]
                observation["top_window_owner_pid"] = top[kCGWindowOwnerPID as String]
                observation["top_window_bounds"] = top[kCGWindowBounds as String]
            }
            focus.append(observation)
            if now >= focusEnd { break }
            try await Task.sleep(nanoseconds: min(100_000_000, focusEnd - now))
        }
        try? await stream.stopCapture()
        queue.sync {}
        var finished = timespec(); clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &finished)
        let cpu = Double(finished.tv_sec - started.tv_sec) + Double(finished.tv_nsec - started.tv_nsec) / 1e9
        let report: [String: Any] = ["scope": "ScreenCaptureKit samples of a test window, not physical scanout", "change_detection": "all content pixels", "hash_scope": "one pixel per 8 by 8 content block", "window": title, "window_id": window.windowID, "capture_cpu_seconds": cpu, "requested_seconds": seconds, "size": [configuration.width, configuration.height], "requested_capture_hz": 120, "frames": recorder.frames, "focus_scope": "approximately 10 Hz owner activity and foremost normal window overlapping content; not continuous visibility or scanout", "focus_observations": focus]
        let data = try JSONSerialization.data(withJSONObject: report, options: [.sortedKeys])
        try data.write(to: URL(fileURLWithPath: output))
        print(title, recorder.frames.count, "capture CPU seconds", cpu)
    }
}
