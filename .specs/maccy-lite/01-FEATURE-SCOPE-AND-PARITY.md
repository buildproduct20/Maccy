# Feature scope, workflows, and behavioral parity

## Features to preserve (R = requirement / T = test id)
| ID | Behavior | Source of old behavior | Acceptance scenario |
|---|---|---|---|
| R01 | Observe macOS general pasteboard without idle busy work | `Maccy/Clipboard.swift` | One external copy becomes one item; self-write never duplicates |
| R02 | Plain text, UTF-8 and Unicode | `Clipboard.swift`, `HistoryItem.swift` | Save, search and restore multi-line Korean/English/emoji text exactly |
| R03 | Images and screenshots | `Clipboard.swift`, `HistoryItem.swift`, image preview views | Capture screenshot, show bounded thumbnail, restore image without lossy conversion |
| R04 | File URLs, multiple file entries | `Clipboard.swift` | Restore multiple file URL pasteboard items as valid file references |
| R05 | Common RTF and HTML plus text fallback | `HistoryItemContent.swift` | Rich copy remains pasteable as appropriate types; no silent stripping |
| R06 | Menu bar status item and click-to-open popup | `AppDelegate.swift`, `FloatingPanel.swift` | Click item -> focused native search popup, no global shortcuts required |
| R07 | Bounded list display and full retained-history search | `History.swift`, `Search.swift` | Search older entry beyond first page; no whole-DB eager hydration |
| R08 | Click item to copy to system clipboard **only** | `Clipboard.copy()`, item view | Clipboard changes; no simulated paste, no Accessibility requirement |
| R09 | Delete selected item and clear all history | `History.swift` | Atomic DB deletion plus eventual orphan-blob cleanup |
| R10 | Configurable max count/age, defaults 200/7d | `StorageSettingsPane.swift` | 201st distinct copy evicts oldest, independent 7d expiry |
| R11 | Ignore apps, types, private content, patterns | `Clipboard.swift`, Ignore Settings | Excluded copies never enter DB, previews or logs |
| R12 | Import supported old settings only | `Defaults.Keys+Names.swift` | Existing Maccy history/settings untouched; import report |
| R13 | Optional launch at login | `GeneralSettingsPane.swift` | Toggle preserved or cleanly disabled if unsafe, tested on target |
| R14 | Native UI interaction and basic accessibility | views, Maccy UI tests | Text search, mouse selection, scrolling, keyboard navigation, accessibility labels |
| R15 | Startup/shutdown robustness | `AppDelegate.swift` | No corrupt database/partial blobs on restart or failed capture |

## Removed intentionally — do not port, do not keep dead code
- All global hotkeys and user-configurable shortcuts, `GlobalHotKey.swift`, `KeyboardShortcuts` usage, shortcut preferences and handlers.
- Pin/favorite model, pin ordering, pinned list UI, pin shortcut allocation, pin special characters.
- OCR/Vision `VNRecognizeTextRequest`, recognition queue, text-from-image search.
- Paste Stack, bulk-paste automation, synthetic `Cmd+V` injection, Accessibility gating for those workflows.
- Notification and sound services; automatic update (Sparkle) and App Store review prompts.
- Advanced appearance options, menu icon customization, floating/slideout auxiliary UI if not required for the minimal popup; keep basic macOS light/dark and readable layout.
- Obsolete migration/compatibility code *specific to removed features*, only after verifying no retained call site uses it.

## Minimal UX journeys
1. Launch -> menu bar icon visible -> status bar menu or click opens history; no Dock window required. Show a simple Settings entry, Clear History and Quit.
2. Copy text in another app -> watcher sees change -> filters/normalizes -> Rust stores -> popup shows concise preview. Same text again -> recency refreshed, no second row.
3. Copy an image -> store original bytes or lossless platform-safe equivalent; show on-demand small thumbnail; clicking old image writes image formats back to pasteboard.
4. Copy several files -> store each file URL in order, without reading private file contents; selecting restores multi-item file references. Do not chase file URLs or pre-read actual files.
5. Search short or long Unicode query -> results across all records, never only loaded 50; press Return or click -> restore selection to clipboard; Escape closes popup.
6. At 200 entries -> capture 201st distinct -> transaction preserves newest 200. At seven days -> startup/periodic cleanup removes expired records even if count < 200.
7. Open Settings -> adjust two retention values + essential content filters -> changes validated and persisted; existing old Maccy settings may be imported with consent.

## Nonfunctional behaviors
- App can coexist with original Maccy but both may monitor the pasteboard; avoid silent self-copy loops and recognize potential duplicated history across apps as separate applications.
- No cloud sync, remote network dependency, telemetry, API keys, update polling or automatic sharing.
- Avoid registering global shortcut, event taps, or permission prompts for paste automation.
- Mouse-click restore-to-clipboard is the default; **not** an automatic paste into the prior app.
- A deleted history record does not clear the macOS system clipboard; it only changes this app's persistent history.
- Original history is neither migrated nor deleted. Old settings import is separate from copy history and requires explicit action.

## Parity requirement for tests
Inventory `MaccyTests/` and `MaccyUITests/` before removal. For each old test, mark **PORT** (same externally visible retained behavior), **REWRITE** (same behavior, new Rust/Swift implementation), or **REMOVE** (feature intentionally gone; state why). Keep an explicit `old-test -> new-test -> executed result` table in the implementation report. A passing Rust unit test does not substitute for macOS pasteboard end-to-end verification. Never claim all original tests will pass unchanged after intentionally removing old features.
