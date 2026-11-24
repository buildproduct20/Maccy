#!/usr/bin/env python3
"""Build and run the Rust/Swift prototype without a real clipboard or Xcode."""
import argparse
import os
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--swift-test", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    swift = (root / args.swift_test).resolve(strict=True)
    if not swift.is_relative_to(root):
        raise ValueError("Native test must be project-local")
    cargo = Path.home() / ".cargo/bin/cargo"
    if not cargo.is_file():
        raise ValueError("Native Rust toolchain is unavailable")
    build = root / "build"
    build.mkdir(exist_ok=True)
    target = build / "rust"
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), MACOSX_DEPLOYMENT_TARGET="14.0")
    manifest = root / "rust/Cargo.toml"
    commands = [
        [str(cargo), "fmt", "--manifest-path", str(manifest), "--all", "--", "--check"],
        [str(cargo), "clippy", "--manifest-path", str(manifest), "--all-targets", "--", "-D", "warnings"],
        [str(cargo), "test", "--manifest-path", str(manifest)],
        [str(cargo), "build", "--manifest-path", str(manifest), "--release", "-p", "maccy_ffi_spike"],
        ["xcrun", "swiftc", "-parse-as-library", "-swift-version", "6", "-import-objc-header",
         str(root / "rust/ffi_spike/include/MaccySpike.h"), str(swift),
         "-L", str(target / "release"), "-lmaccy_ffi_spike",
         "-framework", "AppKit", "-framework", "Foundation", "-framework", "Security",
         "-o", str(build / "native-ffi-spike")],
        [str(build / "native-ffi-spike")],
    ]
    for command in commands:
        result = subprocess.run(command, cwd=root, env=env, check=False)
        if result.returncode:
            return result.returncode
    return 0


if __name__ == "__main__":
    sys.exit(main())
