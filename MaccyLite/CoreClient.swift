import Dispatch
import Foundation

struct HistoryCursor: Codable, Sendable {
  let last_copied_at: Int64
  let recency_order: Int64
  let id: Int64
}

struct HistoryPage: Decodable, Sendable {
  let items: [HistoryRow]
  let next_cursor: HistoryCursor?
  let generation: UInt64
}

struct HistoryRow: Decodable, Sendable, Identifiable {
  let id: Int64
  let kind: String
  let preview: String
  let source_app: String?
  let created_at: Int64
  let last_copied_at: Int64
  let copy_count: UInt64
}

struct LiteSettings: Codable, Sendable {
  var max_items: Int
  var retention_days: Int
  var poll_interval_ms: Int
  var enabled_types: [String]
  var ignored_apps: [String]
  var allowlist_apps: Bool
  var ignored_types: [String]
  var ignored_patterns: [String]
  var paused: Bool
  var import_digest: String?
}

struct ImportResponse: Decodable, Sendable { let report: ImportReport }
struct ImportReport: Decodable, Sendable {
  let imported_keys: [String]
  let skipped_keys: [String]
  let invalid_keys: [String]
  let invalid_entries: Int
  let already_imported: Bool
}
struct CoreOK: Decodable, Sendable { let ok: Bool }
struct CoreGeneration: Decodable, Sendable {
  let generation: UInt64
  let seen_generation: Int64
}
struct CoreThumbnail: Decodable, Sendable { let png: Data }

struct CoreRequest: Encodable, Sendable {
  let op: String
  var query: String? = nil
  var cursor: HistoryCursor? = nil
  var id: Int64? = nil
  var settings: LiteSettings? = nil
  var plist: Data? = nil
  var overwrite: Bool? = nil
  var pixels: UInt32? = nil
}

struct LiteError: LocalizedError, Sendable {
  let code: Int32
  var errorDescription: String? {
    switch code {
    case 1: "The value or format is not supported."
    case 2: "Maccy Lite could not access its private history."
    case 3: "This history item's content is no longer available."
    case 4: "The clipboard operation could not be completed."
    case 5: "Maccy Lite is busy. Try again."
    default: "Maccy Lite could not complete the operation."
    }
  }
}

final class CoreClient: Sendable {
  private let handle: UInt64
  private let slots = DispatchSemaphore(value: 6)
  private let thumbnailSlots = DispatchSemaphore(value: 2)
  private static let queue = DispatchQueue(label: "com.buildproduct20.maccylite.bridge", qos: .userInitiated)

  init(root: URL, pasteboardName: String? = nil) throws {
    try NativeFormats.register()
    let path = Array(root.path.utf8)
    let board = Array((pasteboardName ?? "").utf8)
    var handle: UInt64 = 0
    let code = path.withUnsafeBufferPointer { p in
      board.withUnsafeBufferPointer { b in
        ml_open(p.baseAddress, p.count, b.baseAddress, b.count, &handle)
      }
    }
    guard code == 0 else { throw LiteError(code: code) }
    self.handle = handle
  }

  deinit { _ = ml_close(handle) }

  func request<T: Decodable & Sendable>(_ request: CoreRequest, as type: T.Type) async throws -> T {
    try Task.checkCancellation()
    let slots = request.op == "thumbnail" ? thumbnailSlots : slots
    return try await withCheckedThrowingContinuation { continuation in
      guard slots.wait(timeout: .now()) == .success else {
        continuation.resume(throwing: LiteError(code: 5))
        return
      }
      Self.queue.async { [self] in
        let result = Result { try perform(request, as: type) }
        slots.signal()
        continuation.resume(with: result)
      }
    }
  }

  private func perform<T: Decodable>(_ request: CoreRequest, as type: T.Type) throws -> T {
    let data = try JSONEncoder().encode(request)
    var response = MLBuffer(data: nil, length: 0)
    let code = data.withUnsafeBytes { span in
      ml_request(handle, span.bindMemory(to: UInt8.self).baseAddress, span.count, &response)
    }
    guard code == 0 else { throw LiteError(code: code) }
    guard let bytes = response.data else { throw LiteError(code: 6) }
    defer { ml_free_buffer(bytes, response.length) }
    return try JSONDecoder().decode(type, from: Data(bytes: bytes, count: response.length))
  }
}
