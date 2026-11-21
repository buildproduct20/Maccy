# Rust-first architecture and FFI contract

## Target shape
```text
NSStatusItem + native SwiftUI/AppKit popup (thin Swift presentation)
          |
   Swift view model / observable state adapter
          |
   UniFFI-generated Swift bindings (candidate) OR thin C ABI (fallback)
          |
   Rust AppCore (single owner of lifecycle + state)
     |-- ClipboardService: watcher and macOS pasteboard integration
     |-- HistoryService: insert, dedup, list, search, delete, clear
     |-- RetentionService: count and age policy
     |-- ContentStore: original blobs and lazy thumbnails
     |-- SettingsService: validated Rust configuration
     |-- SQLiteHistoryRepository: transactions, FTS and indexes
     +-- Event channel: minimal lightweight changes -> Swift UI
```

## Responsibilities
Rust owns all application behavior and persistent item state. Swift owns `NSStatusItem`, `NSPanel`, focus and keyboard events while panel open, accessibility labels, SwiftUI layout, and presentation state. Platform API use should move into Rust via `objc2`/`objc2-app-kit` where safe and verified. A tiny Swift adapter for a problematic macOS API (e.g. ImageIO) is allowed if a spike proves superior. Do not duplicate search, dedup, retention or SQLite logic in Swift. Keep a count of remaining Swift adapter surfaces and justify each.

## Proposed Rust workspace
```text
rust/
  Cargo.toml
  maccy_core/
    src/{lib,app,models,settings,errors}.rs
    src/clipboard/{mod,watcher,reader,writer,normalize,macos}.rs
    src/history/{mod,repository,search,dedup,retention}.rs
    src/content/{mod,images,blob_store,thumbs}.rs
    src/ffi/mod.rs
    tests/{history,formats,search,retention,privacy}.rs
  tools/ (if required for synthetic benchmarking)
MaccyLite/
  App.swift, MenuBarController.swift, HistoryView.swift, HistoryViewModel.swift
  SettingsView.swift, NativePlatformAdapter.swift
```
The names are proposed, not mandatory; make the final layout follow the actual project/build structure.

## Initial contract (language-neutral, adapt after spike)
- `AppCore.initialize(config_dir) -> Result<AppHandle, AppError>`: open DB, reconcile prior incomplete blob cleanup, run expiry, start watcher.
- `AppCore.subscribe_events() -> AsyncStream<ItemChange>` or callback under a bounded/coalesced queue; event contains item ID + kind + change type, **not secret payload**.
- `History.list(limit, cursor) -> Result<Page<ItemSummary>>` — summary only; default page 50.
- `History.search(query, limit, cursor) -> Result<Page<ItemSummary>>` — search entire retained DB; short-query fallback.
- `History.get_item(id) -> Result<ClipboardPayload>` — fetch actual content only on restore.
- `History.restore_to_pasteboard(id) -> Result<(), ClipboardError>` — writing to clipboard, **never** synthesizing keystrokes.
- `History.delete(id) / clear() -> Result<...>`.
- `Settings.get() / update(ValidatedSettings)`.
- `Retention.enforce(now) -> CleanupReport`.
- `Image.get_thumbnail(id, max_px) -> Result<ImageDescriptor>` — bounded decode/cache, no duplicate full-sized blob copy.
- `AppCore.shutdown()` — deterministic observer/watcher cancellation and DB close.

`ItemSummary { id, kind, display_text, source_app?, copied_at, image_width?, image_height?, has_thumbnail }`. No `NSImage` or SwiftData model crosses FFI. Avoid passing unbounded bytes through event channels. For thumbnails use a carefully scoped file URL, small owned bytes, or other safe platform adapter, with cache ownership documented.

## Threading and ownership
- Clipboard watcher uses pasteboard change count on macOS, starts with ~500ms polling, sleeps efficiently and does no work on unchanged count.
- Objective-C objects must be accessed on permitted threads and scoped to autorelease pools; audit `objc2` `MainThreadMarker`/thread restrictions rather than assuming Send/Sync.
- SQLite connections are Rust-owned. Prefer a dedicated DB worker and bounded command queue to avoid calls that block the Swift main thread.
- Images use bounded temporary allocations and thumbnail LRU (by bytes, not only count). No full-size image cached in item summaries.
- Send only lightweight events to UI; coalesce repeated changes. Cancel search if query changes; discard stale results by request token/generation.
- Typed FFI errors: no panics/unwinds across FFI. Avoid unchecked lifetimes, raw pointer leaks, double-free and JS/browser runtimes.
- UIKit is not needed. For original SwiftUI code, identify and remove implicit assumptions about SwiftData `@Model`/Observable decorators.
- In crash/DB error, retain last good committed history; do not delete original Maccy anything.

## FFI integration spike — mandatory before large rewrite
1. Inspect real Xcode + Rust toolchain, SDK, deployment target and sandbox permissions.
2. Build minimal Rust staticlib/xcframework callable from existing Swift project (prefer UniFFI if reliable).
3. Prove string/array + error propagation + async/nonblocking call + teardown; run on Apple Silicon.
4. Prove safe native `NSPasteboard` read and write from Rust with tests against disposable named pasteboards (use system pasteboard only in an isolated integration test).
5. If `objc2` clipboard implementation is unsound/unstable, use a tiny Swift AppKit clipboard transport while keeping formatting, dedup, history, storage and watcher state in Rust; document evidence and preserve Rust-first intent.
6. Choose a reproducible build strategy (Xcode build phase or local reproducible helper), don't require runtime toolchain on the installed Mac.
7. Record actual symbols, ABI, ownership and steps in the implementation report.

## Explicit limits
Do not rebuild SwiftUI in Rust; no Tauri/Electron/WebView. Do not preserve classes just because they existed in Maccy; track user-facing feature parity instead. Source is MIT: preserve copyright and attribution. Ad hoc signing for personal local build if supported; no App Store or auto-update mechanism.
