import AppKit
import Carbon
import Foundation

private struct Failure: Error { let check: String }

@main @MainActor
struct LaunchIntegration {
  static func main() async throws {
    let args = CommandLine.arguments
    if args.count == 3 && args[1] == "contender" {
      let root = URL(fileURLWithPath: args[2], isDirectory: true)
      guard case .forwarded = try SingleInstance.start(in: root, onReopen: {}) else {
        throw Failure(check: "contender acquired an existing owner lease")
      }
      return
    }
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
    defer { try? FileManager.default.removeItem(at: root) }
    try await testReopens(root, executable: args[0])
    try testShortcut()
    print("PASS: duplicate launches forward to one owner; popup shortcut registers, reloads and clears")
  }

  private static func testReopens(_ root: URL, executable: String) async throws {
    var count = 0
    guard case let .primary(owner) = try SingleInstance.start(in: root, onReopen: { count += 1 }) else {
      throw Failure(check: "first launch did not become primary")
    }
    defer { withExtendedLifetime(owner) {} }
    for _ in 0..<3 {
      let previous = count
      let contender = Process()
      contender.executableURL = URL(fileURLWithPath: executable)
      contender.arguments = ["contender", root.path]
      try contender.run()
      contender.waitUntilExit()
      guard contender.terminationStatus == 0 else { throw Failure(check: "contender did not forward") }
      let deadline = ContinuousClock.now + .seconds(3)
      while count == previous && ContinuousClock.now < deadline {
        try await Task.sleep(for: .milliseconds(20))
      }
      guard count > previous else { throw Failure(check: "primary did not receive reopen") }
      // Allow any registration-handler notification from the same write to drain.
      try await Task.sleep(for: .milliseconds(40))
    }
  }

  private static func testShortcut() throws {
    _ = NSApplication.shared
    let name = "com.buildproduct20.maccylite.test." + UUID().uuidString
    guard let defaults = UserDefaults(suiteName: name) else { throw Failure(check: "isolated defaults") }
    defer { defaults.removePersistentDomain(forName: name) }
    let first = PopupShortcut(defaults: defaults)
    defer { first.stop() }
    first.start(action: {})
    guard first.key == nil else { throw Failure(check: "must be unset by default") }
    let modifiers = UInt32(controlKey | cmdKey | optionKey | shiftKey)
    let keys = [kVK_F17, kVK_F18, kVK_F19, kVK_F20].enumerated().map { offset, code in
      PopupKey(code: UInt32(code), modifiers: modifiers, keyLabel: "F\(17 + offset)")
    }
    guard let key = keys.first(where: { first.set($0) }) else { throw Failure(check: "Carbon registration") }
    first.stop()
    let reopened = PopupShortcut(defaults: defaults)
    defer { reopened.stop() }
    reopened.start(action: {})
    guard reopened.key == key, reopened.error == nil else { throw Failure(check: "saved shortcut re-registers") }
    reopened.clear()
    let fresh = PopupShortcut(defaults: defaults)
    defer { fresh.stop() }
    fresh.start(action: {})
    guard fresh.key == nil else { throw Failure(check: "removed shortcut is not reloaded") }
  }
}
