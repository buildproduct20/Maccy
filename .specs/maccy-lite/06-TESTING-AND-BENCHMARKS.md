# Test migration, verification and benchmark requirements

## Core rule: translate tests to retained behavior, not line-by-line Swift syntax
Existing baseline test inventory includes `MaccyTests/ClipboardTests.swift`, `HistoryTests.swift`, `HistoryItemTests.swift`, `SearchTests.swift`, `HistoryDecoratorTests.swift`, `SorterTests.swift`, `CollectionSurroundingTests.swift`, `ShortenedTests.swift`, and `MaccyUITests/`. Exact individual test names must be inventoried in a generated parity matrix **before** implementation begins. For each old test, record its specific behavior and disposition `PORT` / `REWRITE` / `REMOVE`, new test path/ID, justification, and executed pass/fail. Removed-feature tests do not remain as expected failures. Relevant preexisting behavior must be verified via equivalent Rust unit/integration tests and macOS end-to-end tests.

## Test layers
- **Rust pure unit:** representation normalization, stable hash input, Unicode search query escaping, ordering, dedup, config validation, safe cutoff clock arithmetic.
- **Rust SQLite integration:** temporary files/DB; 200/201 capacity; duplicate at cap; 7d expiration; batch deletes; FTS coverage; FTS/LIKE fallback; blob reference lifecycle; rollback/restart; transactions and locked DB.
- **macOS native adapter:** independent named pasteboard for normal tests. Verify text, RTF, HTML, image, multiple file URLs; Objective-C object lifecycle; autorelease pools; no synthetic Cmd+V.
- **FFI ownership/concurrency:** Rust→Swift value API, errors, async off-main calls, callback unsubscribe, app teardown and restart, no unsafe object transfer across threads.
- **UI integration:** status icon click; popup search; long history; click-to-restore; manual delete/clear; settings updates and import; keyboard navigation within popup; accessibility names.
- **Crash/privacy:** failed SQLite transaction; malformed blob; clipboard change race; ignored password-manager markers, ignored apps, types, regex; no actual clipboard payload logged.
- **System tests:** launch-at-login if retained; side-by-side install with original Maccy; independent user data directory and Bundle ID.

## Performance baseline
Bench original fork at pinned revision before refactoring, then same synthetic workload after. Profiles must include machine info, macOS version, compiler optimization, how memory was measured, cold/warm start distinction, variance and background process state. Use Instruments/memory footprint where available and macOS RSS as separate metrics. Use **total** footprint if subprocess helpers are used. Run at least three repetitions where feasible; report median and range/P95 with sample counts.

Scenarios: idle 5 minutes; launch with 0, 50, 200 items; synthetic 1k/10k only for benchmark DB at custom limits; 100 consecutive text captures; 20 screenshot captures; close/open popup repeated; search short UTF-8 query/Korean query/exact text; delete/cleanup; restart.

Targets, not guaranteed measurements:
- Idle physical footprint <= 60 MiB **aspirational**, record if infeasible.
- Idle CPU < 1% sustained on an idle macOS desktop.
- Indexed search P95 < 50 ms at 10,000 synthetic records.
- No history-size-proportional object hydration and no unbounded decoded image cache.
- No retained UI jank from database work on main thread.
Never report fabricated benchmarks, conflate DB benchmark with whole-app measurement, or claim production history contains 10k under default 200 max.

## Checklist / DoD
- Every retained feature R01..R15 has at least one linked test.
- Existing test mapping includes explicit individual `old test -> new assertion`.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (adapt to exact workspace) executed and results recorded.
- Xcode app build, macOS UI tests and integration tests run on actual Apple Silicon Mac when available; otherwise mark `BLOCKED: missing macOS/Xcode` with reproducible local commands.
- Prevent tests from accessing original Maccy clipboard history, settings or database; synthetic fixtures only.
- Integration tests that access `NSPasteboard.general` must be isolated/opt-in and restore previous content only when safe, preferably avoid touching real clipboard during automation.
- Check that no unused feature remains wired in menu/settings/keyboard handling; no invalid obsolete imports/dependencies in app project.
- Check release `.app` signature, launch, icons, clipboard interaction, quit and memory with installed build.

## Verification artifacts
`docs/PARITY-MATRIX.md` (generated implementation branch), `docs/PERFORMANCE_RESULTS.md`, `docs/PROGRESS.md`, `docs/HANDOFF.md`. Each test result must include actual command, exit code, OS, timestamp, and observed failures/skips. In final report clearly distinguish **verified**, **not run**, **failed**, and **blocked**.
