import Foundation
import Vision
import AppKit

struct OcrLine: Encodable {
    struct BBox: Encodable {
        let x: Double
        let y: Double
        let w: Double
        let h: Double
    }
    let text: String
    let bbox: BBox
    let confidence: Double
}

func exitWithError(_ message: String) -> Never {
    fputs(message + "\n", stderr)
    exit(1)
}

let args = CommandLine.arguments
guard let imageIndex = args.firstIndex(of: "--image"), imageIndex + 1 < args.count else {
    exitWithError("Usage: vision_ocr.swift --image /path/to/image")
}

let imagePath = args[imageIndex + 1]
guard let image = NSImage(contentsOfFile: imagePath) else {
    exitWithError("Failed to load image: \(imagePath)")
}

guard let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    exitWithError("Failed to create CGImage for: \(imagePath)")
}

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = true
request.minimumTextHeight = 0.012

let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
do {
    try handler.perform([request])
} catch {
    exitWithError("Vision request failed: \(error)")
}

let observations = request.results ?? []
var lines: [OcrLine] = []
lines.reserveCapacity(observations.count)

for observation in observations {
    guard let candidate = observation.topCandidates(1).first else { continue }
    let box = observation.boundingBox
    let bbox = OcrLine.BBox(
        x: Double(box.origin.x),
        y: Double(box.origin.y),
        w: Double(box.size.width),
        h: Double(box.size.height)
    )
    let line = OcrLine(text: candidate.string, bbox: bbox, confidence: Double(candidate.confidence))
    lines.append(line)
}

let encoder = JSONEncoder()
encoder.outputFormatting = [.prettyPrinted]
let data = try encoder.encode(lines)
FileHandle.standardOutput.write(data)
