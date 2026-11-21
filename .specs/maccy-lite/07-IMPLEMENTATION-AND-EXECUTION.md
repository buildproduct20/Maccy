# One-go implementation playbook — PStack / Claude Code

## Intent
The specifications already exist. The implementer should **read and execute them**, not spend its first session creating another speculative set of specs. This is an autonomous engineering assignment, not a plan-only or advisory task. The user wants maximum practical Rust rewrite of *retained* Maccy features and native SwiftUI/AppKit presentation, with tests migrated to equivalent Rust/macOS assertions.

Use actual locally installed PStack `/pstack:poteto-mode` with applicable multi-phase/autonomous-run/perf/refactoring/review playbooks. The coordinator retains state, confirms each step, and delegates independent parts only when merge conflicts and shared Xcode/project/FFI artifacts can be avoided. Do not fake skill invocations or install new global config. Respect original user confirmation boundaries for macOS permissions, Git auth and signing.

## Preconditions
- Repo `buildproduct20/Maccy` forked from `p0deje/Maccy`, baseline `f0b571e32e4031d7b91ba811f50d0f33e8e5548b`.
- Current spec-only branch: `spec/maccy-lite-rust-first`. When implementation starts create an implementation branch based on the latest reviewed spec commit or continue from it in a *separate* implementation branch. Do not blindly overwrite master or original Maccy.
- Check installed Swift, Rust, Apple SDK, Xcode, CI availability; do not assume an Apple-specific build can be run on Linux.
- Read all `.specs/maccy-lite/*.md`; treat user-locked scope as non-negotiable.

## Phases, dependencies and concrete exit checks
| Phase | Objective | Exit gate |
|---|---|---|
| P0 | Snapshot repository, investigate SwiftData and UI/capture paths; enumerate existing tests; run baseline build/tests/profiles | Audit with baseline SHA and actual test+metric evidence |
| P1 | Native Swift-Rust FFI spike, Rust pasteboard reader/writer spike with disposable pasteboard, lifecycle/error/teardown | Proven compilable+tested bridge and actual native pasteboard roundtrip |
| P2 | Write Rust models, SQLite schema, blob staged write/reconcile, config validation | Rust persistence tests D01..D16 where applicable |
| P3 | Rust clipboard watcher/normalizer/writer with private-content filters | macOS adapter test C01..C15, no duplicate self-write |
| P4 | Rust search, dedup, cursor pagination, age+count cleanup, cache | Database/retention/search and restart benchmarks green |
| P5 | Replace SwiftData history binding with Rust summary API; simplify Swift UI and remove obsolete features | Minimal popup, menu bar, restore, delete/clear and settings work |
| P6 | Safe allowlisted settings import from original Maccy without history DB reads | M01..M11 checked; original unchanged |
| P7 | Run full Rust+Xcode/UI test parity; review with PStack; profile and optimize | Traceability table + actual repeated performance results |
| P8 | Independent `.app`, README/license, repeatable packaging, isolated install instructions, PR | Verifiable build output, clear limitations, no automatic master merge |

Implementation and tests are continuous. An early blocker may delay one independent workstream but must not block all other feasible work. Specs may be amended with evidence recorded in decision log.

## Engineering / parallel-agent discipline
- One integration owner for Xcode project and ABI bridge. No two agents edit same schema or FFI contract concurrently.
- Can parallelize: test inventory & expected behaviors, native UI analysis, Rust DB design/tests, CI scripts, benchmarks; integrate serially.
- Use small, reviewable commits with conventional messages; no force push, no automated merge, no global configuration mutations.
- Make red/green checks for critical behaviors. Review diffs against no-feature-regression requirements.
- If a proposed module exists only to increase Rust code ratio while adding unsafe complexity, provide evidence and keep a tiny Swift adapter.
- If prior change is not tested, don't report it complete. Review comments are findings only after reproduction/verification.

## Required final deliverables
1. Independent `Maccy Lite.app` for Apple Silicon or precise non-macOS build blocker with reproduction steps.
2. Actual Rust core crate(s), native Swift shell, UniFFI/C ABI linkage, test suite, `docs/PARITY-MATRIX.md`.
3. `docs/PROGRESS.md`, `docs/HANDOFF.md` and `docs/PERFORMANCE_RESULTS.md` with observed metrics.
4. Clean README showing install, launch, old settings import, known unsupported features, cleanup defaults, rollback and exact manual permissions if needed.
5. Git implementation branch and reviewable PR against `buildproduct20/Maccy:master`, but **do not merge** without explicit instruction.
6. Final Korean report: implemented behavior, removed features, Rust/Swift ownership, test commands and real results, memory before/after with method, app path, Git SHA/PR, unresolved issues.

## Safety gates
Old `org.p0deje.Maccy` files are read-only and **not** to be touched during tests; do not import history. Don't delete or overwrite apps, wipe original DB, modify macOS permissions automatically, or push private settings/secrets. No fabricated performance or successful installation claims. If a user-authenticated system prompt is unavoidable, describe it and finish all other work.

## Paste into Claude Code after checking out spec branch
```text
/pstack:poteto-mode

Implement Maccy Lite end-to-end from the existing specifications at .specs/maccy-lite/README.md and every numbered .md file in that folder.

This is a complete engineering execution, not a spec-writing task. Read the specs, inspect the source and installed PStack capabilities, create an implementation branch from the spec commit, audit baseline and tests, then perform the entire Rust-first rewrite, native SwiftUI/AppKit shell, 200-item + 7-day cleanup, supported original-settings-only import, retained test parity, profiling, packaging and PR preparation.

Honor all requirements and safety gates. No global shortcuts, pins, favorites, OCR, Paste Stack, sound/notifications, updater, App Store code or complex UI. Clicking a history item restores clipboard data only; no automatic paste. Preserve original Maccy unchanged.

Use actual PStack autonomous multi-phase and review workflows, run real tests and measure results, fix failures, write progress and handoff, and continue through delivery wherever system permissions permit. Do not stop after planning, and do not claim blocked tasks are finished.
```
