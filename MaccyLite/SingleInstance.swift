import Darwin
import Dispatch
import Foundation

final class SingleInstance {
  enum Launch { case primary(SingleInstance), forwarded }

  private let source: any DispatchSourceFileSystemObject

  private init(fd: Int32, previous: UInt64?, onReopen: @escaping @MainActor @Sendable () -> Void) {
    source = DispatchSource.makeFileSystemObjectSource(fileDescriptor: fd, eventMask: .write, queue: .main)
    source.setCancelHandler { close(fd) }
    source.setEventHandler { MainActor.assumeIsolated { onReopen() } }
    // Cover writes that land before the kernel finishes arming the source.
    source.setRegistrationHandler {
      if Self.marker(fd) != previous { MainActor.assumeIsolated { onReopen() } }
    }
    source.activate()
  }

  deinit { source.cancel() }

  @MainActor
  static func start(in directory: URL, onReopen: @escaping @MainActor @Sendable () -> Void) throws -> Launch {
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
    let path = directory.appendingPathComponent("instance.lock").path
    let fd = open(path, O_CREAT | O_RDWR | O_NOFOLLOW | O_CLOEXEC, mode_t(0o600))
    guard fd >= 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
    var retainDescriptor = false
    defer { if !retainDescriptor { close(fd) } }
    var info = stat()
    guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFREG, info.st_uid == geteuid(), info.st_nlink == 1 else {
      throw POSIXError(.EINVAL)
    }
    let previous = marker(fd)
    guard flock(fd, LOCK_EX | LOCK_NB) == 0 else {
      guard errno == EWOULDBLOCK else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
      var request = DispatchTime.now().uptimeNanoseconds
      guard pwrite(fd, &request, MemoryLayout<UInt64>.size, 0) == MemoryLayout<UInt64>.size else {
        throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
      }
      return .forwarded
    }
    let instance = SingleInstance(fd: fd, previous: previous, onReopen: onReopen)
    retainDescriptor = true
    return .primary(instance)
  }

  private static func marker(_ fd: Int32) -> UInt64? {
    var value: UInt64 = 0
    return pread(fd, &value, MemoryLayout<UInt64>.size, 0) == MemoryLayout<UInt64>.size ? value : nil
  }
}
