import AppKit
import Carbon
import Observation
import SwiftUI

struct PopupKey: Codable, Equatable {
  let code: UInt32
  let modifiers: UInt32
  let keyLabel: String

  var label: String {
    let flags = [(controlKey, "\u{2303}"), (optionKey, "\u{2325}"), (shiftKey, "\u{21e7}"), (cmdKey, "\u{2318}")]
    return flags.filter { modifiers & UInt32($0.0) != 0 }.map(\.1).joined() + keyLabel
  }
}

@MainActor @Observable
final class PopupShortcut {
  private static let preference = "PopupShortcutV1"
  private static let signature: OSType = 0x4d4c7465
  private let defaults: UserDefaults

  init(defaults: UserDefaults = .standard) { self.defaults = defaults }
  private var registration: EventHotKeyRef?
  private var handler: EventHandlerRef?
  private var serial: UInt32 = 0
  private var activeID: UInt32?
  private var action: (() -> Void)?
  private(set) var key: PopupKey?
  private(set) var error: String?

  func start(action: @escaping () -> Void) {
    self.action = action
    var event = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
    let status = InstallEventHandler(GetApplicationEventTarget(), { _, event, context in
      guard let event, let context else { return OSStatus(eventNotHandledErr) }
      return MainActor.assumeIsolated {
        Unmanaged<PopupShortcut>.fromOpaque(context).takeUnretainedValue().receive(event)
      }
    }, 1, &event, Unmanaged.passUnretained(self).toOpaque(), &handler)
    guard status == noErr else { error = "macOS could not start shortcut registration."; return }
    if let data = defaults.data(forKey: Self.preference),
       let saved = try? JSONDecoder().decode(PopupKey.self, from: data) {
      key = saved
      _ = set(saved)
    }
  }

  @discardableResult
  func set(_ newKey: PopupKey) -> Bool {
    guard handler != nil else { error = "Shortcut registration is unavailable."; return false }
    if key == newKey, registration != nil { error = nil; return true }
    serial &+= 1
    let id = EventHotKeyID(signature: Self.signature, id: serial)
    var replacement: EventHotKeyRef?
    guard RegisterEventHotKey(newKey.code, newKey.modifiers, id, GetApplicationEventTarget(),
                              UInt32(kEventHotKeyExclusive), &replacement) == noErr, let replacement else {
      error = "macOS could not register that shortcut. It may already be in use."
      return false
    }
    if let registration { UnregisterEventHotKey(registration) }
    registration = replacement
    activeID = serial
    key = newKey
    error = nil
    if let data = try? JSONEncoder().encode(newKey) { defaults.set(data, forKey: Self.preference) }
    return true
  }

  func clear() {
    if let registration { UnregisterEventHotKey(registration) }
    registration = nil
    activeID = nil
    key = nil
    error = nil
    defaults.removeObject(forKey: Self.preference)
  }

  func stop() {
    if let registration { UnregisterEventHotKey(registration) }
    if let handler { RemoveEventHandler(handler) }
    registration = nil
    handler = nil
    activeID = nil
    action = nil
  }

  private func receive(_ event: EventRef) -> OSStatus {
    var id = EventHotKeyID()
    let status = GetEventParameter(event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID),
                                   nil, MemoryLayout<EventHotKeyID>.size, nil, &id)
    guard status == noErr, id.signature == Self.signature, id.id == activeID else { return OSStatus(eventNotHandledErr) }
    action?()
    return noErr
  }
}

struct PopupShortcutSettings: View {
  let shortcut: PopupShortcut
  @State private var recording = false

  var body: some View {
    Section("Popup Shortcut") {
      LabeledContent("Open or close history", value: shortcut.key?.label ?? "Not set")
      HStack {
        Button("Record Shortcut...") { recording = true }
        if shortcut.key != nil { Button("Remove Shortcut") { shortcut.clear() } }
      }
      if let error = shortcut.error { Text(error).foregroundStyle(.red) }
      Text("This shortcut opens or closes the history popup. It does not paste.")
        .font(.callout).foregroundStyle(.secondary)
    }
    .sheet(isPresented: $recording) {
      VStack(spacing: 12) {
        Text("Press a shortcut with Command, Control, or Option.").font(.headline)
        ShortcutRecorder { key in
          if let key { _ = shortcut.set(key) }
          recording = false
        }
        .frame(width: 1, height: 1)
        .accessibilityHidden(true)
        Button("Cancel") { recording = false }
      }
      .padding(24)
    }
  }
}

private struct ShortcutRecorder: NSViewRepresentable {
  let finish: (PopupKey?) -> Void

  func makeNSView(context: Context) -> Recorder {
    let view = Recorder()
    view.finish = finish
    return view
  }

  func updateNSView(_ view: Recorder, context: Context) { view.finish = finish }

  final class Recorder: NSView {
    var finish: ((PopupKey?) -> Void)?
    override var acceptsFirstResponder: Bool { true }

    override func viewDidMoveToWindow() {
      super.viewDidMoveToWindow()
      DispatchQueue.main.async { [weak self] in
        guard let self else { return }
        self.window?.makeFirstResponder(self)
      }
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
      if window?.firstResponder === self { return record(event) }
      return super.performKeyEquivalent(with: event)
    }

    override func keyDown(with event: NSEvent) {
      if !record(event) { super.keyDown(with: event) }
    }

    private func record(_ event: NSEvent) -> Bool {
      let flags = event.modifierFlags.intersection([.control, .option, .shift, .command])
      if event.keyCode == kVK_Escape, flags.isEmpty { finish?(nil); return true }
      guard !flags.isDisjoint(with: [.control, .option, .command]) else { return false }
      let label = switch Int(event.keyCode) {
      case kVK_Space: "Space"
      case kVK_Return: "Return"
      case kVK_Tab: "Tab"
      case kVK_Delete: "Delete"
      case kVK_LeftArrow: "Left"
      case kVK_RightArrow: "Right"
      case kVK_UpArrow: "Up"
      case kVK_DownArrow: "Down"
      default: (event.charactersIgnoringModifiers ?? "").uppercased()
      }
      guard !label.isEmpty, !label.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else { return false }
      let modifiers = (flags.contains(.control) ? controlKey : 0) | (flags.contains(.option) ? optionKey : 0)
        | (flags.contains(.shift) ? shiftKey : 0) | (flags.contains(.command) ? cmdKey : 0)
      finish?(PopupKey(code: UInt32(event.keyCode), modifiers: UInt32(modifiers), keyLabel: label))
      return true
    }
  }
}
