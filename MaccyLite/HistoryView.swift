import AppKit
import Observation
import SwiftUI

@MainActor
@Observable
final class LiteHistoryModel {
  private(set) var client: CoreClient?
  var startupFailed = false
  private(set) var isOpen = false
  private(set) var query = ""
  private(set) var rows: [HistoryRow] = []
  private(set) var selectedID: Int64?
  private(set) var isLoading = false
  private(set) var loadFailed = false
  private(set) var isMutating = false
  private(set) var actionError: String?
  var listFocused = false

  private var nextCursor: HistoryCursor?
  var hasMore: Bool { nextCursor != nil }
  @ObservationIgnored private var requestTask: Task<Void, Never>?
  @ObservationIgnored private var generationTask: Task<Void, Never>?
  @ObservationIgnored private var appliedGeneration: UInt64?
  @ObservationIgnored private var opening = UUID()

  func connect(_ client: CoreClient) {
    self.client = client
    if isOpen { refresh(); watchGeneration(client) }
  }

  func open() {
    isOpen = true
    opening = UUID()
    query = ""
    if let client { refresh(); watchGeneration(client) }
  }

  func close() {
    isOpen = false
    requestTask?.cancel()
    generationTask?.cancel()
    requestTask = nil
    generationTask = nil
    rows = []
    selectedID = nil
    listFocused = false
    nextCursor = nil
    appliedGeneration = nil
    isLoading = false
    loadFailed = false
    actionError = nil
  }

  func search(_ value: String) {
    guard value != query else { return }
    query = value
    rows = []
    selectedID = nil
    nextCursor = nil
    actionError = nil
    loadPage(after: nil, debounce: true)
  }

  func refresh() { loadPage(after: nil) }

  func loadMore() {
    guard let nextCursor, !isLoading, !isMutating else { return }
    loadPage(after: nextCursor)
  }

  private func loadPage(after cursor: HistoryCursor?, debounce: Bool = false) {
    guard let client, isOpen else { return }
    requestTask?.cancel()
    let query = self.query
    isLoading = true
    loadFailed = false
    requestTask = Task { [weak self] in
      do {
        if debounce { try await Task.sleep(for: .milliseconds(120)) }
        try Task.checkCancellation()
        let page = try await client.request(CoreRequest(op: "page", query: query, cursor: cursor), as: HistoryPage.self)
        try Task.checkCancellation()
        guard let self else { return }
        if cursor == nil {
          self.rows = page.items
          self.appliedGeneration = page.generation
        } else {
          guard page.generation == self.appliedGeneration else {
            self.refresh()
            return
          }
          let existing = Set(self.rows.map(\.id))
          self.rows.append(contentsOf: page.items.filter { !existing.contains($0.id) })
        }
        self.nextCursor = page.next_cursor
        if !self.rows.contains(where: { $0.id == self.selectedID }) { self.selectedID = self.rows.first?.id }
        self.isLoading = false
      } catch {
        guard !Task.isCancelled else { return }
        self?.loadFailed = true
        self?.isLoading = false
      }
    }
  }

  private func watchGeneration(_ client: CoreClient) {
    generationTask?.cancel()
    generationTask = Task { [weak self] in
      while !Task.isCancelled {
        do { try await Task.sleep(for: .milliseconds(750)) } catch { return }
        guard self?.isLoading == false, self?.isMutating == false, self?.loadFailed == false else { continue }
        guard let change = try? await client.request(CoreRequest(op: "generation"), as: CoreGeneration.self),
          !Task.isCancelled, let self, self.isOpen, !self.isLoading, !self.isMutating,
          change.generation != self.appliedGeneration
        else { continue }
        self.refresh()
      }
    }
  }

  func moveSelection(_ direction: Int) {
    guard !rows.isEmpty else { return }
    let current = rows.firstIndex { $0.id == selectedID }
    let target = current.map { min(max($0 + direction, 0), rows.count - 1) } ?? 0
    selectedID = rows[target].id
  }

  func restore(_ id: Int64, dismiss: @escaping @MainActor () -> Void) {
    guard rows.contains(where: { $0.id == id }) else { return }
    perform(CoreRequest(op: "restore", id: id), failure: "Could not copy this item to the clipboard.", then: dismiss)
  }

  func delete(_ id: Int64) {
    guard rows.contains(where: { $0.id == id }) else { return }
    perform(CoreRequest(op: "delete", id: id), failure: "Could not delete this item.")
  }

  func clear() {
    perform(CoreRequest(op: "clear"), failure: "Could not clear clipboard history.")
  }

  private func perform(
    _ request: CoreRequest, failure: String, then: (@MainActor () -> Void)? = nil
  ) {
    guard let client, isOpen, !isMutating else { return }
    let opening = self.opening
    isMutating = true
    actionError = nil
    Task { [weak self] in
      let result = try? await client.request(request, as: CoreOK.self)
      guard let self else { return }
      self.isMutating = false
      guard self.isOpen, self.opening == opening else { return }
      guard result?.ok == true else {
        self.actionError = failure
        return
      }
      if let then { then() } else { self.refresh() }
    }
  }
}

struct LiteHistoryView: View {
  static let cornerRadius: CGFloat = if #available(macOS 26, *) { 7 } else { 4 }

  let model: LiteHistoryModel
  let retryStartup: () -> Void
  let dismiss: @MainActor () -> Void
  let openSettings: () -> Void
  let clearHistory: () -> Void
  let quit: () -> Void

  @FocusState private var searchFocused: Bool
  @FocusState private var listFocused: Bool

  var body: some View {
    VStack(spacing: 0) {
      searchField.padding(5)
      Divider().padding(.horizontal, 6)
      historyContent
      if let error = model.actionError {
        Text(error).font(.callout).foregroundStyle(.secondary).padding(5)
      }
      Divider().padding(.horizontal, 6)
      footer.padding(5)
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .onChange(of: model.isOpen, initial: true) { _, isOpen in
      searchFocused = isOpen
      if !isOpen { listFocused = false }
    }
    .onChange(of: listFocused) { _, focused in model.listFocused = focused }
  }

  private var searchField: some View {
    HStack(spacing: 6) {
      Image(systemName: "magnifyingglass")
        .font(.system(size: 11))
        .foregroundStyle(.secondary)
        .accessibilityHidden(true)
      TextField("Search clipboard history", text: Binding(get: { model.query }, set: { query in model.search(query) }))
        .disableAutocorrection(true)
        .textFieldStyle(.plain)
        .focused($searchFocused)
        .onSubmit(restoreSelection)
        .accessibilityIdentifier("history-search")
      if !model.query.isEmpty {
        Button {
          model.search("")
          searchFocused = true
        } label: {
          Image(systemName: "xmark.circle.fill")
            .font(.system(size: 11))
            .foregroundStyle(.secondary)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Clear search")
      }
    }
    .padding(.horizontal, 5)
    .frame(height: 23)
    .background(Color.secondary.opacity(0.1), in: RoundedRectangle(cornerRadius: Self.cornerRadius))
  }

  @ViewBuilder private var historyContent: some View {
    if model.startupFailed {
      status("Could not open Maccy Lite history.", retry: retryStartup)
    } else if model.rows.isEmpty {
      if model.loadFailed {
        status("Could not load clipboard history.", retry: model.refresh)
      } else if model.client == nil || model.isLoading {
        ProgressView().controlSize(.small).accessibilityLabel("Loading history")
          .frame(maxWidth: .infinity, maxHeight: .infinity)
      } else {
        status(model.query.isEmpty ? "No clipboard history yet." : "No matching items.")
      }
    } else {
      rowsView
    }
  }

  private func status(_ message: String, retry: (() -> Void)? = nil) -> some View {
    VStack(spacing: 8) {
      Text(message).foregroundStyle(.secondary)
      if let retry { Button("Try Again", action: retry) }
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
  }

  private var rowsView: some View {
    ScrollViewReader { proxy in
      ScrollView {
        LazyVStack(spacing: 1) {
          ForEach(model.rows) { row in
            LiteHistoryRowView(row: row, client: model.client, selected: model.selectedID == row.id) {
              model.restore(row.id, dismiss: dismiss)
            } delete: {
              model.delete(row.id)
            }
            .id(row.id)
          }
          if model.loadFailed {
            Text("Could not load more items.").foregroundStyle(.secondary).padding(4)
          }
          if model.hasMore {
            Button(model.isLoading ? "Loading…" : "Load More", action: model.loadMore)
              .buttonStyle(.borderless)
              .disabled(model.isLoading)
              .padding(5)
          } else if model.loadFailed {
            Button("Try Again", action: model.refresh).padding(5)
          }
        }
        .padding(5)
      }
      .focusable(interactions: .edit)
      .focused($listFocused)
      .onKeyPress(.return, phases: .down) { _ in
        restoreSelection()
        return .handled
      }
      .onChange(of: model.selectedID) { _, id in
        if let id { proxy.scrollTo(id) }
      }
    }
  }

  private var footer: some View {
    HStack(spacing: 8) {
      Button("Delete Item") {
        if let id = model.selectedID { model.delete(id) }
      }
      .disabled(model.selectedID == nil || model.isMutating)
      Button("Clear History…", action: clearHistory)
        .disabled(model.client == nil || model.isMutating)
      Spacer(minLength: 0)
      Button("Settings…", action: openSettings).disabled(model.client == nil)
      Button("Quit", action: quit)
    }
    .buttonStyle(.borderless)
    .controlSize(.small)
  }

  private func restoreSelection() {
    if let id = model.selectedID { model.restore(id, dismiss: dismiss) }
  }
}

private struct LiteHistoryRowView: View {
  let row: HistoryRow
  let client: CoreClient?
  let selected: Bool
  let restore: () -> Void
  let delete: () -> Void

  @State private var thumbnail: NSImage?
  @State private var hovered = false

  var body: some View {
    Button(action: restore) {
      HStack(spacing: 6) {
        Group {
          if let thumbnail {
            Image(nsImage: thumbnail).resizable().scaledToFit()
          } else {
            Image(systemName: symbol)
          }
        }
        .frame(width: 24, height: 20)
        .accessibilityHidden(true)
        Text(verbatim: title)
          .lineLimit(1)
          .truncationMode(.tail)
      }
      .frame(maxWidth: .infinity, minHeight: row.kind == "image" ? 28 : 23, alignment: .leading)
      .padding(.horizontal, 5)
      .foregroundStyle(selected ? Color.white : Color.primary)
      .background(
        selected ? Color.accentColor.opacity(0.85) : Color.secondary.opacity(hovered ? 0.12 : 0.001),
        in: RoundedRectangle(cornerRadius: LiteHistoryView.cornerRadius, style: .continuous)
      )
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .onHover { hovered = $0 }
    .accessibilityLabel(Text(verbatim: "\(kindLabel): \(title)"))
    .accessibilityHint("Copies this item to the clipboard")
    .accessibilityAddTraits(selected ? .isSelected : [])
    .accessibilityIdentifier("copy-history-item")
    .accessibilityAction(named: Text("Delete Item"), delete)
    .task(id: row.id) {
      guard row.kind == "image", let client else { return }
      guard let result = try? await client.request(CoreRequest(op: "thumbnail", id: row.id, pixels: 64), as: CoreThumbnail.self),
        !Task.isCancelled
      else { return }
      thumbnail = NSImage(data: result.png)
    }
    .onDisappear { thumbnail = nil }
  }

  private var title: String { row.preview.isEmpty ? kindLabel : row.preview }

  private var kindLabel: String {
    switch row.kind {
    case "image": "Image"
    case "files": "Files"
    case "rich": "Rich Text"
    default: "Text"
    }
  }

  private var symbol: String {
    switch row.kind {
    case "image": "photo"
    case "files": "doc.on.doc"
    case "rich": "doc.richtext"
    default: "doc.plaintext"
    }
  }
}
