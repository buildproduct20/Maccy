import AppKit
import SwiftUI

@main
@MainActor
struct MaccyLiteApp {
  static func main() {
    let application = NSApplication.shared
    application.setActivationPolicy(.accessory)
    do {
      let support = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: false)
      let root = support.appendingPathComponent("com.buildproduct20.maccylite", isDirectory: true)
      let delegate = LiteAppDelegate(root: root)
      let launch = try SingleInstance.start(in: root) { [weak delegate] in delegate?.reopenHistory() }
      guard case let .primary(instance) = launch else { return }
      application.delegate = delegate
      withExtendedLifetime((delegate, instance)) { application.run() }
    } catch {
      let alert = NSAlert()
      alert.messageText = "Maccy Lite could not start"
      alert.informativeText = "Its private data directory or instance lock is unavailable."
      alert.runModal()
    }
  }
}

@MainActor
private final class LiteAppDelegate: NSObject, NSApplicationDelegate {
  private let root: URL
  private let history = LiteHistoryModel()
  private let shortcut = PopupShortcut()
  private var pendingReopen = false

  init(root: URL) { self.root = root }
  private var statusItem: NSStatusItem?
  private var historyPanel: LiteHistoryPanel?
  private var settingsWindow: NSWindow?
  private var startupTask: Task<Void, Never>?

  func applicationDidFinishLaunching(_ notification: Notification) {
    let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
    statusItem = item
    if let button = item.button {
      button.image = NSImage(systemSymbolName: "clipboard", accessibilityDescription: "Maccy Lite")
      button.image?.isTemplate = true
      button.toolTip = "Maccy Lite"
      button.setAccessibilityLabel("Maccy Lite Clipboard History")
      button.target = self
      button.action = #selector(toggleHistory)
    }
    shortcut.start { [weak self] in self?.toggleHistory() }
    startCore()
    if pendingReopen { reopenHistory() }
  }

  func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
    reopenHistory()
    return false
  }

  func applicationWillTerminate(_ notification: Notification) {
    shortcut.stop()
  }

  private func startCore() {
    guard startupTask == nil, history.client == nil else { return }
    history.startupFailed = false
    startupTask = Task { [weak self, root] in
      defer { self?.startupTask = nil }
      do {
        let client = try await Task.detached(priority: .userInitiated) {
          try CoreClient(root: root)
        }.value
        if !Task.isCancelled { self?.history.connect(client) }
      } catch {
        if !Task.isCancelled { self?.history.startupFailed = true }
      }
    }
  }

  @objc private func toggleHistory() {
    if historyPanel?.isVisible == true {
      historyPanel?.dismiss()
    } else {
      reopenHistory()
    }
  }

  func reopenHistory() {
    guard let button = statusItem?.button else { pendingReopen = true; return }
    pendingReopen = false
    if let panel = historyPanel, panel.isVisible {
      panel.orderFrontRegardless()
      panel.makeKey()
      return
    }
    let panel = historyPanel ?? makeHistoryPanel()
    historyPanel = panel
    history.open()
    panel.present(at: button)
    button.highlight(true)
  }

  private func makeHistoryPanel() -> LiteHistoryPanel {
    let panel = LiteHistoryPanel(model: history) { [weak self] in
      self?.history.close()
      self?.statusItem?.button?.highlight(false)
    }
    let view = LiteHistoryView(
      model: history,
      retryStartup: { [weak self] in self?.startCore() },
      dismiss: { [weak panel] in panel?.dismiss() },
      openSettings: { [weak self] in self?.openSettings() },
      clearHistory: { [weak self] in self?.confirmClear() },
      quit: { NSApp.terminate(nil) }
    )
    panel.install(view)
    return panel
  }

  private func confirmClear() {
    guard let panel = historyPanel, panel.isVisible, panel.attachedSheet == nil, !history.isMutating else { return }
    let alert = NSAlert()
    alert.alertStyle = .warning
    alert.messageText = "Clear clipboard history?"
    alert.informativeText = "This permanently deletes all items saved by Maccy Lite. It does not clear the system clipboard."
    alert.addButton(withTitle: "Cancel")
    alert.addButton(withTitle: "Clear History").hasDestructiveAction = true
    alert.beginSheetModal(for: panel) { [weak self] response in
      if response == .alertSecondButtonReturn { self?.history.clear() }
    }
  }

  private func openSettings() {
    guard let client = history.client else { return }
    historyPanel?.dismiss()
    let window: NSWindow
    if let existing = settingsWindow {
      window = existing
    } else {
      window = NSWindow(
        contentRect: NSRect(x: 0, y: 0, width: 500, height: 580),
        styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false
      )
      window.title = "Maccy Lite Settings"
      window.isReleasedWhenClosed = false
      window.contentMinSize = NSSize(width: 430, height: 410)
      window.contentView = NSHostingView(rootView: LiteSettingsView(client: client, shortcut: shortcut))
      window.center()
      settingsWindow = window
    }
    NSApp.activate()
    window.makeKeyAndOrderFront(nil)
  }
}

@MainActor
private final class LiteHistoryPanel: NSPanel {
  private let model: LiteHistoryModel
  private let onDismiss: () -> Void
  private let popupSize = NSSize(width: 380, height: 408)

  init(model: LiteHistoryModel, onDismiss: @escaping () -> Void) {
    self.model = model
    self.onDismiss = onDismiss
    super.init(
      contentRect: NSRect(origin: .zero, size: popupSize),
      styleMask: [.titled, .fullSizeContentView, .nonactivatingPanel], backing: .buffered, defer: false
    )
    title = "Maccy Lite Clipboard History"
    titleVisibility = .hidden
    titlebarAppearsTransparent = true
    titlebarSeparatorStyle = .none
    isMovable = false
    isFloatingPanel = true
    hidesOnDeactivate = false
    level = .statusBar
    collectionBehavior = [.moveToActiveSpace, .fullScreenAuxiliary]
    animationBehavior = .none
    isOpaque = false
    backgroundColor = .clear
    hasShadow = true
    standardWindowButton(.closeButton)?.isHidden = true
    standardWindowButton(.miniaturizeButton)?.isHidden = true
    standardWindowButton(.zoomButton)?.isHidden = true
  }

  override var canBecomeKey: Bool { true }

  func install(_ view: LiteHistoryView) {
    let background = NSVisualEffectView()
    background.material = .popover
    background.blendingMode = .behindWindow
    background.state = .active
    background.wantsLayer = true
    background.layer?.cornerCurve = .continuous
    background.layer?.cornerRadius = LiteHistoryView.cornerRadius + 5
    background.layer?.masksToBounds = true

    let content = NSHostingView(rootView: view.ignoresSafeArea())
    content.translatesAutoresizingMaskIntoConstraints = false
    background.addSubview(content)
    NSLayoutConstraint.activate([
      content.leadingAnchor.constraint(equalTo: background.leadingAnchor),
      content.trailingAnchor.constraint(equalTo: background.trailingAnchor),
      content.topAnchor.constraint(equalTo: background.topAnchor),
      content.bottomAnchor.constraint(equalTo: background.bottomAnchor),
    ])
    contentView = background
  }

  func present(at button: NSStatusBarButton) {
    let anchor = button.window?.convertToScreen(button.convert(button.bounds, to: nil))
    let fallback = NSEvent.mouseLocation
    let visible = (button.window?.screen ?? NSScreen.main)?.visibleFrame
    let size = visible.map { NSSize(width: min($0.width, popupSize.width), height: min($0.height, popupSize.height)) }
      ?? popupSize
    var origin = NSPoint(x: anchor?.minX ?? fallback.x, y: (anchor?.minY ?? fallback.y) - size.height)
    if let visible {
      origin.x = min(max(origin.x, visible.minX), visible.maxX - size.width)
      origin.y = min(max(origin.y, visible.minY), visible.maxY - size.height)
    }
    setFrame(NSRect(origin: origin, size: size), display: true)
    orderFrontRegardless()
    makeKey()
  }

  func dismiss() {
    guard model.isOpen, attachedSheet == nil else { return }
    onDismiss()
    orderOut(nil)
  }

  override func resignKey() {
    super.resignKey()
    if attachedSheet == nil, NSApp.modalWindow == nil { dismiss() }
  }

  override func sendEvent(_ event: NSEvent) {
    // Only this panel owns these keys. Leave Tab, editing, and IME composition to AppKit.
    if event.type == .keyDown, attachedSheet == nil,
      event.modifierFlags.isDisjoint(with: [.shift, .control, .option, .command]),
      (firstResponder as? NSTextInputClient)?.hasMarkedText() != true
    {
      if event.keyCode == 53 {
        dismiss()
        return
      }
      if (firstResponder as? NSTextView)?.isFieldEditor == true || model.listFocused {
        switch event.keyCode {
        case 125: model.moveSelection(1); return
        case 126: model.moveSelection(-1); return
        default: break
        }
      }
    }
    super.sendEvent(event)
  }
}
