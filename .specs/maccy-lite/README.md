# Maccy Lite — Rust-first engineering specification

**Status:** IMPLEMENTATION SPECIFICATION, not implemented. **Owner:** personal-use fork `buildproduct20/Maccy`.
**Base source revision:** `f0b571e32e4031d7b91ba811f50d0f33e8e5548b` (Maccy 2.x master, 2026-11-21 inspection).
**Primary document:** This index. Read every numbered document before changing code.

## Product contract
Rewrite the *retained behavior* of Maccy in Rust, not a blind line-by-line translation. Keep a thin native SwiftUI/AppKit presentation layer where that is more stable; Rust must own clipboard capture/write-back where practical, persistence, search, deduplication, item lifetime, retention, and application core. No Tauri, Electron, WebView, Node, cloud services, or external backend.

The user explicitly chose: (1) click an item **restores it to the system clipboard only**, and does **not** synthesize Cmd+V; (2) **no shortcuts**, including global, custom and pin shortcuts; (3) no pins/favorites, OCR, Paste Stack, sound, notifications, auto-update, App Store review/distribution, complex UI customization; (4) use their original Maccy **settings** where supported, but do not import old history; (5) both count-based and age-based retention: maximum **200** items and **7 days** for text, images, files, and rich text.

**Critical nuance:** Keep ordinary keyboard text entry, Tab, arrow navigation, Return and Escape inside the open native popup (these are native controls, not programmable shortcuts). Menu-bar click opens the popup. No global keyboard hook or simulated paste; Accessibility permission should not be requested unless a separately retained feature really needs it.

## Nonnegotiable acceptance
- Original `org.p0deje.Maccy` installation, application data, settings, clipboard history and sandbox container untouched.
- Independent app identity suggested: `com.buildproduct20.maccylite`; fresh private DB; personal Apple Silicon build.
- Efficient clipboard operations and bounded Rust-owned metadata/thumbnail caches.
- On insertion at the 200-item ceiling, retain the new unique record and evict the oldest by `last_copied_at`. Duplicate refresh changes recency and does not trigger gratuitous eviction.
- Independently evict records whose `last_copied_at` is older than seven days; both constraints apply. Expiration also happens at startup and in light periodic cleanup.
- Search *all retained records*, not merely the on-screen page.
- Compatible preference import via an explicit, safe, permission-respecting mechanism; never import old history or obsolete shortcut/pin settings.
- Automated tests for retained behaviors must be **ported or replaced with equivalent Rust/Swift/macOS tests**, and run. Tests for removed behavior are removed **only after mapping and explaining their loss**.
- Instrument baseline and post-change measurements. Report verified numbers or an explicit "not measured"; never fabricate them.
- A buildable independent `.app` and a reviewable implementation branch are delivery goals, not assumed outputs.

## Document map
1. `01-FEATURE-SCOPE-AND-PARITY.md` — retained/removed features and traceability.
2. `02-RUST-ARCHITECTURE-AND-FFI.md` — module boundaries, API, ownership, concurrency.
3. `03-CLIPBOARD-FORMATS-AND-PRIVACY.md` — copy semantics, data protection, error modes.
4. `04-SQLITE-SEARCH-RETENTION.md` — schema, transactions, search, bounded cleanup.
5. `05-SETTINGS-IMPORT-AND-IDENTITY.md` — preferences, sandbox, app identity.
6. `06-TESTING-AND-BENCHMARKS.md` — parity test strategy and reproducible metrics.
7. `07-IMPLEMENTATION-AND-EXECUTION.md` — PStack phases, checkpoints, release.
8. `08-SOURCE-AUDIT-AND-DECISIONS.md` — real Maccy code findings and fixed decisions.

## Priority order and engineering philosophy
P0 correctness, privacy and recoverability > P1 reduced **total** idle footprint > P2 maximum **practical** Rust-owned runtime functionality > P3 low idle CPU > P4 UX responsiveness > P5 code simplicity. Rust code percentage is not a valid outcome metric. Prefer Rust for core, objc2 for platform bridging where safe, Swift only for genuinely better native functionality. Use a quick FFI and clipboard integration spike before large rewrites.

This specification is intentionally independent of the installed local machine and agent stack. The implementer must first examine actual `git status`, Xcode, Swift, Rust, PStack and plugin versions; never assume a capability was installed or a performance claim was verified.

## Specification change process
If an implementation-dependent decision must change, update this source-of-truth in the implementation branch and `08-SOURCE-AUDIT-AND-DECISIONS.md` with evidence. Do not silently reintroduce removed features. Continue independently actionable work if macOS signing, permissions, Xcode, or authentication needs the user.

## Start instruction
Use `07-IMPLEMENTATION-AND-EXECUTION.md` as the execution entry point. Do not stop at planning; this repository already contains the requested plan. Do not repeat a PRD-only phase.
