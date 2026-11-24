#!/usr/bin/env python3
"""Build an isolated, ad-hoc signed Apple Silicon Maccy Lite app with macOS CLT."""
import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "build/rust"
TRIPLE = "aarch64-apple-darwin"
APP = ROOT / "MaccyLite"


def run(*args, env=None):
    subprocess.run(args, cwd=ROOT, env=env, check=True)


def link(output, sources):
    run("xcrun", "swiftc", "-parse-as-library", "-swift-version", "6", "-O",
        "-target", "arm64-apple-macosx14.0", "-import-objc-header",
        str(ROOT / "rust/maccy_core/include/MaccyCore.h"),
        *map(str, sources), "-L", str(TARGET / TRIPLE / "release"), "-lmaccy_core",
        "-framework", "AppKit", "-framework", "Foundation", "-framework", "Security",
        "-framework", "ImageIO", "-framework", "CoreGraphics",
        "-framework", "Carbon", "-framework", "ServiceManagement", "-o", str(output))


def icon(build, output):
    assets = ROOT / "Maccy/Assets.xcassets/AppIcon.appiconset"
    iconset = build / "AppIcon.iconset"
    iconset.mkdir()
    for entry in json.loads((assets / "Contents.json").read_text())["images"]:
        scale = "@2x" if entry["scale"] == "2x" else ""
        shutil.copyfile(assets / entry["filename"], iconset / ("icon_" + entry["size"] + scale + ".png"))
    run("iconutil", "-c", "icns", "-o", str(output), str(iconset))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test", action="store_true", help="Run disposable native clipboard and launch regressions")
    args = parser.parse_args()
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        parser.error("This build requires an Apple Silicon Mac and Command Line Tools")
    build = ROOT / "build"
    build.mkdir(exist_ok=True)
    cargo = shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")
    run(cargo, "build", "--manifest-path", str(ROOT / "rust/Cargo.toml"),
        "--locked", "--release", "--target", TRIPLE, "-p", "maccy_core",
        env=dict(os.environ, CARGO_TARGET_DIR=str(TARGET), MACOSX_DEPLOYMENT_TARGET="14.0"))
    bridge = [APP / "CoreClient.swift", APP / "NativeFormats.swift"]
    if args.test:
        with tempfile.TemporaryDirectory(prefix="maccy-native-test-", dir=build) as temp:
            executable = Path(temp) / "native-clipboard-tests"
            link(executable, bridge + [ROOT / "native-tests/ClipboardIntegration.swift"])
            run(str(executable))
            launch_test = Path(temp) / "native-launch-tests"
            link(launch_test, [APP / "SingleInstance.swift", APP / "PopupShortcut.swift",
                               ROOT / "native-tests/LaunchIntegration.swift"])
            run(str(launch_test))
        return
    # Never overwrite any installed or previously built app.
    package = Path(tempfile.mkdtemp(prefix="lite-release-", dir=build))
    app = package / "Maccy Lite.app"
    contents = app / "Contents"
    (contents / "MacOS").mkdir(parents=True)
    (contents / "Resources").mkdir()
    shutil.copyfile(APP / "Info.plist", contents / "Info.plist")
    shutil.copyfile(ROOT / "LICENSE", contents / "Resources/LICENSE")
    icon(package, contents / "Resources/AppIcon.icns")
    link(contents / "MacOS/MaccyLite", sorted(APP.glob("*.swift")))
    run("codesign", "--sign", "-", "--entitlements", str(APP / "MaccyLite.entitlements"), str(app))
    run("codesign", "--verify", "--strict", str(app))
    print(app)


if __name__ == "__main__":
    main()
