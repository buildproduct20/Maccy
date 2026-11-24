import CoreGraphics
import Foundation
import ImageIO
import AppKit
import UniformTypeIdentifiers

// The callbacks operate on values only and are called by the Rust owner thread.
enum NativeFormats {
  private static let registration = ml_register_native(liteProcessFormat, liteReleaseFormat)

  static func register() throws {
    guard registration == 0 else { throw LiteError(code: registration) }
  }
}

private func liteReleaseFormat(_ bytes: UnsafeMutablePointer<UInt8>?, _ length: Int) {
  bytes?.deallocate()
}

private func liteProcessFormat(
  _ operation: UInt32, _ bytes: UnsafePointer<UInt8>?, _ length: Int, _ pixels: UInt32,
  _ result: UnsafeMutablePointer<MLNativeOutput>?
) -> Int32 {
  guard let bytes, let result, (1...32 * 1024 * 1024).contains(length) else { return 1 }
  return autoreleasepool {
    let data = Data(bytesNoCopy: UnsafeMutableRawPointer(mutating: bytes), count: length, deallocator: .none)
    func output(_ data: Data) -> Int32 {
      guard data.count <= 2 * 1024 * 1024 else { return 1 }
      if data.isEmpty { return 0 }
      let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: data.count)
      data.copyBytes(to: buffer, count: data.count)
      result.pointee.data = buffer
      result.pointee.length = data.count
      return 0
    }
    if operation == 1 {
      guard data.count <= 2 * 1024 * 1024,
            let text = NSAttributedString(rtf: data, documentAttributes: nil)?.string else { return 1 }
      return output(Data(text.utf8))
    }
    guard operation == 2 || operation == 3,
          let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
          let raw = CGImageSourceGetType(source) as String?,
          [UTType.png.identifier, UTType.tiff.identifier, UTType.jpeg.identifier, UTType.heic.identifier].contains(raw),
          let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
          let width = (properties[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue,
          let height = (properties[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue,
          (1...8192).contains(width), (1...8192).contains(height), width * height <= 64_000_000 else { return 1 }
    if operation == 2 {
      result.pointee.width = UInt32(width)
      result.pointee.height = UInt32(height)
      return 0
    }
    guard (16...128).contains(pixels), let thumbnail = CGImageSourceCreateThumbnailAtIndex(source, 0, [
      kCGImageSourceCreateThumbnailFromImageAlways: true,
      kCGImageSourceThumbnailMaxPixelSize: min(max(width, height), Int(pixels)),
      kCGImageSourceCreateThumbnailWithTransform: true,
      kCGImageSourceShouldCacheImmediately: true
    ] as CFDictionary) else { return 1 }
    let encoded = NSMutableData()
    guard let destination = CGImageDestinationCreateWithData(encoded, UTType.png.identifier as CFString, 1, nil) else { return 1 }
    CGImageDestinationAddImage(destination, thumbnail, nil)
    guard CGImageDestinationFinalize(destination) else { return 1 }
    result.pointee.width = UInt32(thumbnail.width)
    result.pointee.height = UInt32(thumbnail.height)
    return output(encoded as Data)
  }
}
