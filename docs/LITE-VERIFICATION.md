# Maccy Lite verification

This fork adds a separate Apple Silicon menu-bar app; it does not replace the original Maccy target. The app opens one Rust-owned private SQLite history store, captures supported pasteboard formats, restores clipboard bytes without pasting, and defaults to 200 items and seven days. The Swift shell offers one opt-in popup shortcut and a system-managed login item. No original settings are read without explicit user selection; no original history is imported.

## Reproduce

Run from the repository root on an Apple Silicon Mac with Rust and the macOS Command Line Tools:

```sh
cargo test --manifest-path rust/Cargo.toml --locked --workspace
python3 -I scripts/build_lite.py --test
python3 -I scripts/build_lite.py
```

The Rust suite covers retention, search including Unicode and pagination, duplicate recency, blob integrity, rollback and settings import. The macOS suite uses a named pasteboard, a disposable database, three separate duplicate-launch processes and isolated shortcut preferences. The build verifies the local ad-hoc signature and prints the app path. No signing identity, network runtime, background helper, or Rust toolchain is required to use the built app.

## Remaining verification

The automated suite does not exercise recording a key chord in the Settings window, pressing it from another application, or a full logout/login cycle. Full XCTest UI automation, complete original-test parity, and before/after whole-app memory and CPU profiling are not established. A foreground app is not proof of who wrote to the general pasteboard; app-ID filters skip capture when the writer cannot be trusted. This limitation is shown in Settings.
