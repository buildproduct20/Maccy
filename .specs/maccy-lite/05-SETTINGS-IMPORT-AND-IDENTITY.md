# Personal-use identity and compatible Maccy settings import

## Identity / coexistence
Existing original Maccy bundle ID: `org.p0deje.Maccy`. New app ID suggestion: `com.buildproduct20.maccylite`; independent private data directory and separately named app. Do not overwrite `/Applications/Maccy.app`, original sandbox container, original `Storage.sqlite`, or old UserDefaults. No existing history import; start with an empty new DB even if original history exists. Preserve MIT license/attribution.

Original settings are declared in `Maccy/Extensions/Defaults.Keys+Names.swift`, mainly `Defaults` keys using `UserDefaults.standard` within original sandbox. The settings importer must not assume `UserDefaults(suiteName: "org.p0deje.Maccy")` can simply read that sandbox from the new bundle. Establish actual storage and permission behavior on the target machine.

## Whitelisted candidate setting mapping
| Old key | New key / behavior | Policy |
|---|---|---|
| `enabledPasteboardTypes` | captured content types | Import supported types, validate recognized UTIs |
| `ignoredApps` | excluded source app IDs | Import known string list, validate |
| `ignoreAllAppsExceptListed` | allowlist mode if retained | Preserve semantics if supported, else clearly skip |
| `ignoreRegexp` | exclusion regex list | Import with bounded length; compile/test safely, skip invalid expressions individually |
| `ignoredPasteboardTypes` | excluded clipboard type strings | Import, preserving concealed/transient safety defaults |
| `clipboardCheckInterval` | watcher polling interval | Clamp to safe minimum/maximum; default ~500ms |
| `searchMode` | search behavior | Import only if semantic mapping exists |
| `showInStatusBar` | status item visibility | May be skipped: app requires accessible entry via menu bar |
| `size` key stored as `historySize` | max items | **DO NOT import**; new default 200 requested |
| `clearOnQuit`, `clearSystemClipboard` | no automatic clipboard deletion | Skip by default for safe predictable behavior |
| `pasteByDefault`, `removeFormattingByDefault` | old automatic paste settings | Skip; click-to-copy only |
| `pinTo`, `pinOrder`, `showSpecialSymbols` | removed | Skip |
| `popupPosition`, `windowSize`, `menuIcon`, `showFooter`, `showTitle` | simplified UI | Skip unless strictly meaningful |
| keyboard shortcut data in KeyboardShortcuts | removed | Skip |
| `lastReviewRequestedAt`, `numberOfUsages`, `migrations` | removed | Skip |

All newly introduced retention settings start at **200 items / 7 days**. Avoid reading/modifying system user preferences without a clear authorized path.

## Import mechanism
1. On explicit user request ("Import Maccy Settings"), attempt OS-permitted, read-only access to compatible original preference values; do not probe or open the original clipboard-history DB.
2. If sandbox blocks access, offer an **explicit user-approved export/import** flow (e.g. documented `defaults export`/plist flow or GUI-selected plist, with path validation). Never disable sandbox to work around it.
3. Parse into typed temporary structure, whitelist keys, validate values, show/record imported/skipped/invalid key counts. No clipboard content or real path values in logs.
4. Apply as a single validated settings update; respect current settings already changed in Maccy Lite, prefer user confirmation before replacing them.
5. Persist new-format settings in Maccy Lite's private location; idempotent import marker keyed to source/settings version, and optional explicit re-import.
6. If original Maccy is running, new app may coexist with distinct state. Both might monitor clipboard; no destructive interaction, no global shortcut conflicts (Maccy Lite has none).
7. No silent fake success. A blocked import is a user-action item but must not block the rest of the app.

## Simplified settings UI
- Maximum history items: default 200, validated positive integer (choose sensible upper bound, e.g. 10k).
- Retention days: default 7, validated 1..365 or another documented bound.
- Capture content types (text/image/files).
- Ignore app/type/pattern controls if retained.
- Launch at login (optional, avoid false migration of OS registration).
- Import Old Maccy Settings button and outcome.
Do not implement shortcut, pin, OCR, updater or complex appearance preferences.

## Tests
`M01` import from synthetic plist fixture; `M02` deny access without touching original; `M03` skip keys for removed features; `M04` malformed regex/type safe fallback; `M05` preserved privacy defaults; `M06` idempotence; `M07` existing new prefs not overwritten unexpectedly; `M08` count=200 and retention=7 unaffected; `M09` no old history DB access; `M10` new app data and app identity fully independent; `M11` import report accurately distinguishes imported/skipped/blocked.
