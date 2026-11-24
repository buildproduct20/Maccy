import AppKit
import Foundation

private struct CheckFailed: Error { let assertion: String }

private func check(_ condition: Bool, _ assertion: String) throws {
  if !condition { throw CheckFailed(assertion: assertion) }
}

@main
struct ClipboardIntegration {
  @MainActor
  static func main() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let name = "com.buildproduct20.maccylite.test." + UUID().uuidString
    let board = NSPasteboard(name: NSPasteboard.Name(name))
    defer { board.releaseGlobally() }
    let copied = try await scenario(root, name: name, board: board)
    try await reopen(root, name: name, board: board, copied: copied)
    let cleared = try CoreClient(root: root, pasteboardName: name)
    _ = try await history(cleared, count: 0)
    print("PASS: named pasteboard text, rich formats, image thumbnail, file URLs, privacy, restore and reopen")
  }

  @MainActor
  private static func scenario(_ root: URL, name: String, board: NSPasteboard) async throws -> Data {
    let core = try CoreClient(root: root, pasteboardName: name)
    var settings = try await core.request(.init(op: "settings"), as: LiteSettings.self)
    try check(settings.max_items == 200 && settings.retention_days == 7, "initial retention")
    settings.poll_interval_ms = 100
    let _: CoreOK = try await core.request(.init(op: "update_settings", settings: settings), as: CoreOK.self)

    let text = "\u{D55C}\u{AE00} \u{1F680}\r\nnext"
    try write(board, [[.string: Data(text.utf8)]])
    let textPage = try await history(core, count: 1)
    let textRow = textPage.items[0]
    let found = try await core.request(.init(op: "page", query: "\u{D55C}"), as: HistoryPage.self)
    try check(found.items.first?.id == textRow.id, "short Unicode search")

    let html = Data("<p>Hello &amp; welcome</p>".utf8)
    try write(board, [[.html: html]])
    let htmlPage = try await history(core, count: 2)
    let htmlRow = htmlPage.items[0]
    try check(htmlRow.preview == "Hello & welcome", "HTML fallback")
    let _: CoreOK = try await core.request(.init(op: "restore", id: htmlRow.id), as: CoreOK.self)
    try check(board.data(forType: .html) == html && board.string(forType: .string) == "Hello & welcome", "HTML original and derived text restore")
    try await Task.sleep(for: .milliseconds(300))
    let after = try await history(core, count: 2)
    try check(after.items[0].copy_count == 2, "self-write updates recency exactly once")

    let rtf = Data(#"{\rtf1\ansi Hello RTF}"#.utf8)
    try write(board, [[.rtf: rtf, .string: Data("Hello RTF".utf8)]])
    let rich = try await history(core, count: 3)
    let _: CoreOK = try await core.request(.init(op: "restore", id: rich.items[0].id), as: CoreOK.self)
    try check(board.data(forType: .rtf) == rtf && board.string(forType: .string) == "Hello RTF", "RTF representation restore")

    guard let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 32, pixelsHigh: 16, bitsPerSample: 8,
                                        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                                        bytesPerRow: 0, bitsPerPixel: 0) else { throw CheckFailed(assertion: "image fixture") }
    for y in 0..<16 {
      for x in 0..<32 { bitmap.setColor(NSColor(deviceRed: 0, green: 0.4, blue: 1, alpha: 1), atX: x, y: y) }
    }
    guard let image = bitmap.representation(using: .png, properties: [:]) else { throw CheckFailed(assertion: "PNG fixture") }
    try write(board, [[.png: image]])
    let imagePage = try await history(core, count: 4)
    let imageRow = imagePage.items[0]
    let thumb = try await core.request(.init(op: "thumbnail", id: imageRow.id, pixels: 16), as: CoreThumbnail.self)
    let decoded = NSBitmapImageRep(data: thumb.png)
    try check(decoded?.pixelsWide == 16 && decoded?.pixelsHigh == 8, "bounded thumbnail aspect ratio")
    let _: CoreOK = try await core.request(.init(op: "restore", id: imageRow.id), as: CoreOK.self)
    try check(board.data(forType: .png) == image, "PNG original bytes")

    let files = ["file:///synthetic/a.txt", "file:///synthetic/b.txt"]
    try write(board, files.map { [.fileURL: Data($0.utf8)] })
    let filePage = try await history(core, count: 5)
    let fileRow = filePage.items[0]
    let _: CoreOK = try await core.request(.init(op: "restore", id: fileRow.id), as: CoreOK.self)
    try check(board.pasteboardItems?.compactMap { $0.string(forType: .fileURL) } == files, "ordered file references")

    let concealed = NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType")
    try write(board, [[.string: Data("excluded fixture".utf8), concealed: Data([1])]])
    try await waitForCapture(core, generation: board.changeCount)
    let privatePage = try await history(core, count: 5)
    try check(privatePage.items.allSatisfy { !$0.preview.contains("excluded fixture") }, "concealed item not stored")

    settings.enabled_types = [NSPasteboard.PasteboardType.html.rawValue]
    settings.ignored_patterns = ["private-fixture"]
    let _: CoreOK = try await core.request(.init(op: "update_settings", settings: settings), as: CoreOK.self)
    try write(board, [[.string: Data("private-fixture".utf8), .html: Data("<b>benign HTML</b>".utf8)]])
    try await waitForCapture(core, generation: board.changeCount)
    _ = try await history(core, count: 5)
    try write(board, [[.html: Data("<b>Public HTML</b>".utf8)]])
    _ = try await history(core, count: 6)

    let _: CoreOK = try await core.request(.init(op: "restore", id: textRow.id), as: CoreOK.self)
    let copied = Data(text.utf8)
    try check(board.data(forType: .string) == copied, "exact UTF-8 and line endings restore")
    return copied
  }

  @MainActor
  private static func reopen(_ root: URL, name: String, board: NSPasteboard, copied: Data) async throws {
    let core = try CoreClient(root: root, pasteboardName: name)
    let page = try await history(core, count: 6)
    try check(page.items[0].kind == "text", "restored recency survives reopen")
    let settings = try await core.request(.init(op: "settings"), as: LiteSettings.self)
    try check(settings.ignored_patterns == ["private-fixture"], "settings survive reopen")
    let _: CoreOK = try await core.request(.init(op: "clear"), as: CoreOK.self)
    _ = try await history(core, count: 0)
    try check(board.data(forType: .string) == copied, "clear history leaves clipboard intact")
  }

  private static func waitForCapture(_ core: CoreClient, generation: Int) async throws {
    for _ in 0..<40 {
      let state = try await core.request(.init(op: "generation"), as: CoreGeneration.self)
      if state.seen_generation == Int64(generation) { return }
      try await Task.sleep(for: .milliseconds(100))
    }
    throw CheckFailed(assertion: "poller did not observe the clipboard generation")
  }

  private static func history(_ core: CoreClient, count: Int) async throws -> HistoryPage {
    for _ in 0..<40 {
      let page = try await core.request(.init(op: "page"), as: HistoryPage.self)
      if page.items.count == count { return page }
      try await Task.sleep(for: .milliseconds(100))
    }
    throw CheckFailed(assertion: "history count did not converge")
  }

  @MainActor
  private static func write(_ board: NSPasteboard, _ payload: [[NSPasteboard.PasteboardType: Data]]) throws {
    let items = try payload.map { values in
      let item = NSPasteboardItem()
      for (kind, data) in values { try check(item.setData(data, forType: kind), "fixture representation write") }
      return item
    }
    board.clearContents()
    try check(board.writeObjects(items), "fixture clipboard write")
  }
}
