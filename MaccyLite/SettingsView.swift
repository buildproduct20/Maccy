import AppKit
import Darwin
import SwiftUI
import UniformTypeIdentifiers

struct LiteSettingsView: View {
  let client: CoreClient
  let shortcut: PopupShortcut

  @State private var draft: LiteSettingsDraft?
  @State private var saved: LiteSettingsDraft?
  @State private var busy = false
  @State private var choosingFile = false
  @State private var importURL: URL?
  @State private var confirmingImport = false
  @State private var overwrite = false
  @State private var report: ImportReport?
  @State private var message: String?
  @State private var failed = false

  var body: some View {
    Form {
      PopupShortcutSettings(shortcut: shortcut)
      LoginItemSettings()
      if let draft = Binding($draft) {
        LiteSettingsFields(draft: draft).disabled(busy)
        Section("Legacy Maccy Settings") {
          Toggle("Replace settings already changed in Maccy Lite", isOn: $overwrite)
          Button("Import Maccy Settings...") { choosePlist() }
          Text("Choose a Maccy preferences plist you exported. Only supported settings are imported, never clipboard history.")
            .font(.callout).foregroundStyle(.secondary)
        }
        .disabled(busy || choosingFile)
      }

      if let report {
        Section("Import Result") {
          if report.already_imported {
            Text("These settings were already imported. No changes were made.")
          } else {
            importKeys("Imported", report.imported_keys)
            importKeys("Skipped", report.skipped_keys)
            importKeys("Invalid", report.invalid_keys)
            if report.invalid_entries > 0 { Text("Invalid entries: \(report.invalid_entries)") }
          }
        }
      }

      Section {
        if busy { ProgressView().controlSize(.small).accessibilityLabel("Working") }
        if let message {
          Text(message).foregroundStyle(failed ? Color.primary : Color.secondary)
        }
        if let draft {
          HStack {
            Spacer()
            Button("Save Settings") { save(draft) }
              .disabled(busy || choosingFile || draft == saved)
          }
        } else if !busy && failed {
          Button("Try Again") { Task { await reload() } }
        }
      }
    }
    .formStyle(.grouped)
    .task { if draft == nil { await reload() } }
    .alert("Import Maccy settings?", isPresented: $confirmingImport) {
      Button("Cancel", role: .cancel) { importURL = nil }
      Button("Import Settings") {
        guard let url = importURL else { return }
        importURL = nil
        importSettings(from: url, overwrite: overwrite)
      }
    } message: {
      Text(importPrompt)
    }
  }

  private var importPrompt: String {
    let policy = overwrite
      ? "Supported imported values can replace changes already saved in Maccy Lite."
      : "Maccy Lite keeps supported settings you've already changed."
    return draft == saved ? policy : policy + " Unsaved edits in this window will be discarded if import succeeds."
  }

  private func importKeys(_ label: String, _ keys: [String]) -> some View {
    VStack(alignment: .leading, spacing: 2) {
      Text("\(label): \(keys.count)")
      if !keys.isEmpty {
        Text(verbatim: keys.prefix(8).joined(separator: ", ") + (keys.count > 8 ? ", ..." : ""))
          .font(.caption).foregroundStyle(.secondary)
      }
    }
  }

  private func reload() async {
    guard !busy else { return }
    busy = true
    defer { busy = false }
    do {
      try await readSettings()
      message = nil
      failed = false
    } catch {
      guard !Task.isCancelled else { return }
      showError("Could not load Maccy Lite settings.")
    }
  }

  private func readSettings() async throws {
    let settings = try await client.request(CoreRequest(op: "settings"), as: LiteSettings.self)
    try Task.checkCancellation()
    let loaded = LiteSettingsDraft(settings)
    draft = loaded
    saved = loaded
  }

  private func save(_ draft: LiteSettingsDraft) {
    guard !busy else { return }
    guard let settings = draft.settings else {
      showError("Enter whole numbers in the ranges shown for history and polling.")
      return
    }
    busy = true
    message = nil
    failed = false
    Task {
      defer { busy = false }
      let result = try? await client.request(CoreRequest(op: "update_settings", settings: settings), as: CoreOK.self)
      guard result?.ok == true else {
        showError("Could not save settings. Check the capture filters and regular expressions, then try again.")
        return
      }
      await reloadAfterChange("Settings saved.")
    }
  }

  private func choosePlist() {
    guard !busy, !choosingFile else { return }
    let panel = NSOpenPanel()
    panel.title = "Choose a Maccy Settings Plist"
    panel.prompt = "Choose"
    panel.allowedContentTypes = [.propertyList]
    panel.canChooseFiles = true
    panel.canChooseDirectories = false
    panel.allowsMultipleSelection = false
    panel.resolvesAliases = false
    choosingFile = true
    panel.begin { response in
      choosingFile = false
      guard response == .OK, let url = panel.url else { return }
      importURL = url
      confirmingImport = true
    }
  }

  private func importSettings(from url: URL, overwrite: Bool) {
    guard !busy else { return }
    busy = true
    message = nil
    failed = false
    report = nil
    Task {
      defer { busy = false }
      let data: Data
      do {
        data = try await Task.detached(priority: .userInitiated) { try readImportPlist(url) }.value
      } catch {
        showError("Could not read the selected file. Choose a regular .plist file no larger than 2 MiB.")
        return
      }
      do {
        let result = try await client.request(
          CoreRequest(op: "import_settings", plist: data, overwrite: overwrite), as: ImportResponse.self
        )
        report = result.report
        if !result.report.already_imported { await reloadAfterChange("Settings import finished.") }
      } catch {
        showError("Could not import the selected preferences. No settings were changed.")
      }
    }
  }

  private func reloadAfterChange(_ success: String) async {
    do {
      try await readSettings()
      message = success
    } catch {
      draft = nil
      saved = nil
      showError(success + " Could not reload the updated settings.")
    }
  }

  private func showError(_ text: String) {
    message = text
    failed = true
  }
}

private struct LiteSettingsFields: View {
  @Binding var draft: LiteSettingsDraft

  var body: some View {
    Section("Retention") {
      numberField("Maximum history items", value: $draft.maxItems, range: "1 to 10,000")
      numberField("Retention days", value: $draft.retentionDays, range: "1 to 365")
      Text("Reducing either limit may remove saved items.").font(.callout).foregroundStyle(.secondary)
    }
    Section("Capture") {
      Toggle("Pause clipboard capture", isOn: $draft.paused)
      numberField("Polling interval (ms)", value: $draft.pollInterval, range: "100 to 5,000")
      captureType("Plain Text", uti: "public.utf8-plain-text")
      captureType("File URLs", uti: "public.file-url")
      captureType("HTML", uti: "public.html")
      captureType("Rich Text (RTF)", uti: "public.rtf")
      captureType("PNG Images", uti: "public.png")
      captureType("TIFF Images", uti: "public.tiff")
      captureType("JPEG Images", uti: "public.jpeg")
      captureType("HEIC Images", uti: "public.heic")
    }
    Section("Privacy Filters") {
      Toggle("Capture only from listed apps", isOn: $draft.allowlistApps)
      linesField(draft.allowlistApps ? "Allowed app bundle IDs" : "Ignored app bundle IDs", value: $draft.apps)
      linesField("Ignored pasteboard types", value: $draft.ignoredTypes)
      linesField("Ignored text patterns (regular expressions)", value: $draft.ignoredPatterns)
      Text("Enter one value per line. macOS privacy markers are always excluded.")
        .font(.callout).foregroundStyle(.secondary)
      Text("macOS does not identify the clipboard writer. Setting an app filter pauses capture rather than risk saving excluded content.")
        .font(.callout).foregroundStyle(.secondary)
    }
  }

  private func numberField(_ label: String, value: Binding<String>, range: String) -> some View {
    LabeledContent {
      TextField(label, text: value, prompt: Text(range))
        .labelsHidden()
        .frame(width: 100)
    } label: {
      Text(label)
      Text(range)
    }
  }

  private func linesField(_ label: String, value: Binding<String>) -> some View {
    VStack(alignment: .leading, spacing: 4) {
      Text(label)
      TextEditor(text: value)
        .disableAutocorrection(true)
        .font(.system(.body, design: .monospaced))
        .frame(height: 45)
        .border(Color.secondary.opacity(0.25))
        .accessibilityLabel(Text(label))
    }
  }

  private func captureType(_ label: String, uti: String) -> some View {
    Toggle(label, isOn: Binding(get: { draft.types.contains(uti) }, set: { enabled in
      if enabled { draft.types.insert(uti) } else { draft.types.remove(uti) }
    }))
  }
}

private struct LiteSettingsDraft: Equatable {
  var maxItems: String
  var retentionDays: String
  var pollInterval: String
  var types: Set<String>
  var apps: String
  var allowlistApps: Bool
  var ignoredTypes: String
  var ignoredPatterns: String
  var paused: Bool

  private let originalTypes: [String]
  private let originalApps: [String]
  private let originalIgnoredTypes: [String]
  private let originalPatterns: [String]
  private let importDigest: String?

  init(_ settings: LiteSettings) {
    maxItems = String(settings.max_items)
    retentionDays = String(settings.retention_days)
    pollInterval = String(settings.poll_interval_ms)
    types = Set(settings.enabled_types)
    apps = settings.ignored_apps.joined(separator: "\n")
    allowlistApps = settings.allowlist_apps
    ignoredTypes = settings.ignored_types.joined(separator: "\n")
    ignoredPatterns = settings.ignored_patterns.joined(separator: "\n")
    paused = settings.paused
    originalTypes = settings.enabled_types
    originalApps = settings.ignored_apps
    originalIgnoredTypes = settings.ignored_types
    originalPatterns = settings.ignored_patterns
    importDigest = settings.import_digest
  }

  var settings: LiteSettings? {
    guard let max = Int(maxItems), (1...10_000).contains(max),
      let days = Int(retentionDays), (1...365).contains(days),
      let interval = Int(pollInterval), (100...5_000).contains(interval)
    else { return nil }
    return LiteSettings(
      max_items: max, retention_days: days, poll_interval_ms: interval,
      enabled_types: types == Set(originalTypes) ? originalTypes : types.sorted(),
      ignored_apps: apps == originalApps.joined(separator: "\n") ? originalApps : lines(apps),
      allowlist_apps: allowlistApps,
      ignored_types: ignoredTypes == originalIgnoredTypes.joined(separator: "\n") ? originalIgnoredTypes : lines(ignoredTypes),
      ignored_patterns: ignoredPatterns == originalPatterns.joined(separator: "\n") ? originalPatterns : lines(ignoredPatterns, trim: false),
      paused: paused, import_digest: importDigest
    )
  }

  private func lines(_ text: String, trim: Bool = true) -> [String] {
    text.split(whereSeparator: \.isNewline)
      .map { trim ? $0.trimmingCharacters(in: .whitespaces) : String($0) }
      .filter { !$0.isEmpty }
  }
}

private enum ImportFileError: Error { case invalidFile }

private func readImportPlist(_ url: URL) throws -> Data {
  let limit = 2 * 1024 * 1024
  guard url.isFileURL, url.pathExtension.lowercased() == "plist" else { throw ImportFileError.invalidFile }
  let hasAccess = url.startAccessingSecurityScopedResource()
  defer { if hasAccess { url.stopAccessingSecurityScopedResource() } }

  // Reject links and non-regular files at open time; a FIFO must not block the reader.
  let fd = url.withUnsafeFileSystemRepresentation { path in
    guard let path else { return Int32(-1) }
    return Darwin.open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK)
  }
  guard fd >= 0 else { throw ImportFileError.invalidFile }
  let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
  defer { try? handle.close() }
  var info = stat()
  guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFREG, info.st_size > 0, info.st_size <= limit else {
    throw ImportFileError.invalidFile
  }

  var contents = Data()
  while let chunk = try handle.read(upToCount: min(64 * 1024, limit + 1 - contents.count)), !chunk.isEmpty {
    contents.append(chunk)
    guard contents.count <= limit else { throw ImportFileError.invalidFile }
  }
  guard !contents.isEmpty else { throw ImportFileError.invalidFile }
  return contents
}
