import Foundation

struct SpikeFailure: Error {
  let message: String
}

func require(_ condition: Bool, _ message: String) throws {
  if !condition { throw SpikeFailure(message: message) }
}

func roundtrip(_ handle: UInt64, _ input: Data) throws -> Data {
  try require(!Thread.isMainThread, "FFI should run off the UI thread")
  var required = 0
  let sizeCode = input.withUnsafeBytes { bytes in
    ml_spike_roundtrip(handle, bytes.bindMemory(to: UInt8.self).baseAddress,
                       bytes.count, nil, 0, &required)
  }
  try require(sizeCode == 4 && required == input.count, "size/error contract")
  var output = Data(count: required)
  let capacity = required
  let code = input.withUnsafeBytes { bytes in
    output.withUnsafeMutableBytes { destination in
      ml_spike_roundtrip(handle, bytes.bindMemory(to: UInt8.self).baseAddress,
                         bytes.count, destination.bindMemory(to: UInt8.self).baseAddress,
                         capacity, &required)
    }
  }
  try require(code == 0 && required == input.count, "roundtrip contract")
  return output
}

@main
struct NativeFFISpike {
  @MainActor
  static func main() async throws {
    try require(ml_spike_named_pasteboard() == 0, "Rust disposable named pasteboard")
    let handle = ml_spike_open()
    try require(handle != 0, "handle creation")
    defer { _ = ml_spike_close(handle) }

    let strings = ["plain", "line\nnext", "\u{D55C}\u{AE00}", "\u{1F680}"]
    let payload = try JSONEncoder().encode(strings)
    let echoed = try await Task.detached {
      try roundtrip(handle, payload)
    }.value
    try require(try JSONDecoder().decode([String].self, from: echoed) == strings,
                "Unicode string-array fidelity")

    var count = 0
    let invalid = Data("{}".utf8).withUnsafeBytes { bytes in
      ml_spike_roundtrip(handle, bytes.bindMemory(to: UInt8.self).baseAddress,
                         bytes.count, nil, 0, &count)
    }
    try require(invalid == 3, "typed invalid-payload error")
    let temporary = ml_spike_open()
    try require(ml_spike_close(temporary) == 0, "deterministic teardown")
    try require(ml_spike_close(temporary) == 1, "stale handle rejection")
    print("PASS: native Rust named pasteboard, Swift C ABI, Unicode arrays, typed errors, off-main call, teardown")
  }
}
