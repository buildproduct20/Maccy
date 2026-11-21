# Baseline code audit and architectural decisions

**Scope:** Source inspected in public fork `buildproduct20/Maccy` / upstream `p0deje/Maccy`, revision `f0b571e32e4031d7b91ba811f50d0f33e8e5548b`. These are **source observations**, not measured runtime results.

## Source findings
- `Maccy/Observables/History.swift` uses `FetchDescriptor<HistoryItem>()` without a fetch limit in `load()`, decorates and stores entire result in memory. It keeps `allUnpinnedItems` and `pinManager`. Filtered search is applied to in-memory decorators. Replace by Rust DB-backed bounded pages and full retained DB search.
- `Maccy/Models/HistoryItemContent.swift` `@Model` stores `value: Data?` attached to model relationship. Refactor to disk-backed original content and metadata-only listing.
- `Maccy/Models/HistoryItem.swift` stores an `NSImage` cached decoded image and calls Vision OCR in `generateTitle()` for images. Remove OCR entirely; eliminate unbounded decoded image lifetime. Avoid preloading image bytes on startup.
- `Maccy/Observables/HistoryItemDecorator.swift` retains preview/thumbnail `NSImage` and generation tasks, and checks image presence by evaluating `item.image`. Replace with lightweight item summary and bounded on-demand thumbnail cache.
- `Maccy/Clipboard.swift` uses macOS `NSPasteboard`, `Timer`, `changeCount` and reads multiple types across pasteboard items; capture may hold sizeable `HistoryItemContent` arrays. Preserve multi-format semantics, push watcher/transform/write logic into Rust after native spike.
- `Maccy/Search.swift` operates on arrays in memory; replace with indexed DB query, protecting shorter Unicode search.
- `Maccy/Storage.swift` has full `FetchDescriptor<HistoryItemContent>()` for obsolete-content removal and `FetchDescriptor<HistoryItem>()` for title sanitization. New Rust DB should make retention/repair batched and indexed.
- `Maccy/Settings/StorageSettingsPane.swift` exposes 1..999 history size and type toggles. New settings simplified to max-count + retention-days + essential privacy/content filters.
- `Maccy/Extensions/Defaults.Keys+Names.swift` declares original preference keys and defaults: `historySize=200`, `clipboardCheckInterval=0.5`; see settings spec for allowlist.
- `Maccy/Extensions/KeyboardShortcuts.Name+Shortcuts.swift` declares old `popup`, `pin`, `delete`, `togglePreview`. All custom/global hotkey registration is removed by request.
- `Maccy/Settings/GeneralSettingsPane.swift` currently uses `KeyboardShortcuts`, `LaunchAtLogin` and auto-updater settings. Retain only practical launch-at-login and core config.
- `Maccy/AppDelegate.swift` integrates `Sparkle`, status bar UI, panel and history hooks. Remove updater, keep status bar/popup without global shortcut.
- Existing tests: `MaccyTests/` has behavior suites for clipboard/history/search/models/decorators; `MaccyUITests/` exists. Full per-test inventory still required when implementation begins.

## Upstream context and no-blind-cherry-pick
- Upstream `p0deje/Maccy` release v2.8.0 (2026-10-26) includes fixes for history item memory leaks, orphan contents, CPU slowdown and Swift 6 migration; don't regress known fixes.
- PR #1441 (merged July 2026) included decoded image caching, lazy preview and avoidance of extra fetches; understand before rewriting.
- PR #1446 (open as of November 2026) suggests reducing memory by 50-entry fetch and `externalStorage` and claims large local footprint improvement. Its review notes that a 50-row-only fetch can break search across older records. We must maintain search of all retained rows and measure rather than copy claims.
- Maccy is MIT-licensed; retain notices. Fork currently shares base revision with upstream.

## Decisions locked by user
ADR-001: Personal Apple Silicon target; independent Bundle ID and data directory.
ADR-002: Maximum practical Rust engine, thin Swift native UI, no WebView.
ADR-003: No configurable/global shortcuts or pins/favorites.
ADR-004: No image OCR, Paste Stack, sound/notifications, auto-update, App Store or complex appearance.
ADR-005: Clicking history item copies to system clipboard only; user manually pastes into target app.
ADR-006: New empty history; import only compatible old settings; original Maccy untouched.
ADR-007: Both count 200 and age 7 days apply equally to all stored kinds; duplicate refresh and oldest eviction.
ADR-008: Native keyboard text editing/navigation inside the popup is not a configurable global shortcut and remains acceptable.
ADR-009: Reuse and migrate retained behavior tests; eliminate only removed-feature assertions with traceability.
ADR-010: Optimize overall app memory, not an arbitrary Rust LOC percentage; safely retain tiny Swift platform adapter when justified.

## Decisions to validate empirically
- ADR-P1: UniFFI vs stable C ABI on actual toolchain, with compilation+memory safety spike.
- ADR-P2: ObjC clipboard implementation in Rust vs tiny native Swift transport on thread-safe integration evidence.
- ADR-P3: macOS Apple image framework bridge vs Rust image library for lossless content and memory footprint.
- ADR-P4: FTS5 trigram availability on deployment environment; short-query and multilingual fallback.
- ADR-P5: Exact install identity, launch-on-login registration, code-signing and sandbox permissions.

## Baseline acceptance
This file is source audit only; no claim that project builds, tests pass, or RAM target is met. The implementation must gather measured baseline and post-implementation evidence in `docs/PERFORMANCE_RESULTS.md`, plus explicit per-feature test parity.
