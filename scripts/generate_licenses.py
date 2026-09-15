"""
Collect third-party license metadata for the About page.

Reads the resolved npm tree (package-lock.json) and the Rust manifest, and
writes a compact list to src-tauri/resources/licenses.json. Run after changing
dependencies:  python scripts/generate_licenses.py
"""
from __future__ import annotations

import json
import os
import re

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")

# Runtime tools the app invokes as external processes rather than links against.
EXTERNAL_TOOLS = [
    {
        "name": "yt-dlp",
        "version": "managed at runtime",
        "license": "Unlicense",
        "kind": "external-tool",
        "url": "https://github.com/yt-dlp/yt-dlp",
        "note": "Used to read public metadata and stream URLs. Invoked as a separate process; never linked into the app.",
    },
    {
        "name": "FFmpeg",
        "version": "managed at runtime",
        "license": "GPL-3.0-or-later (managed build) / varies for a custom build",
        "kind": "external-tool",
        "url": "https://ffmpeg.org",
        "note": "Used only to merge separate audio/video streams and for user-requested conversion. Invoked as a separate process, never linked. The copy this app installs is the GPL build from yt-dlp/FFmpeg-Builds; a custom path may point at any other build. The Android app ships a GPL build of FFmpeg 7.1 and ffprobe inside the APK instead.",
    },
    {
        "name": "Python",
        "version": "3.12 (Android only)",
        "license": "PSF-2.0",
        "kind": "external-tool",
        "url": "https://www.python.org",
        "note": "Ships inside the Android app to run yt-dlp, which is a Python program. Invoked as a separate process; never linked into the app.",
    },
    {
        "name": "QuickJS",
        "version": "Android only",
        "license": "MIT",
        "kind": "external-tool",
        "url": "https://bellard.org/quickjs/",
        "note": "Ships inside the Android app so yt-dlp can run the JavaScript some sites require before they list their formats. Invoked as a separate process.",
    },
    {
        "name": "youtubedl-android",
        "version": "0.18.1 (Android only)",
        "license": "GPL-3.0",
        "kind": "external-tool",
        "url": "https://github.com/JunkFood02/youtubedl-android",
        "note": "Source of the Android builds of Python, FFmpeg and QuickJS listed above, which it packages from Termux. Only those binaries are used; its source and the sources of the builds are available from the project.",
    },
]

FONTS = [
    {
        "name": "Inter",
        "version": "variable",
        "license": "OFL-1.1",
        "kind": "font",
        "url": "https://rsms.me/inter/",
    }
]

ASSETS = [
    {
        "name": "Simple Icons",
        "version": "16.31.0",
        "license": "CC0-1.0",
        "kind": "asset",
        "url": "https://simpleicons.org",
        "note": "Source of the platform logos on the source badges. The icon data is public domain; the logos themselves are trademarks of their owners and are shown only to name the site a link is from.",
    }
]


def npm_packages() -> list[dict]:
    lock_path = os.path.join(ROOT, "package-lock.json")
    with open(lock_path, encoding="utf-8") as fh:
        lock = json.load(fh)

    pkg_path = os.path.join(ROOT, "package.json")
    with open(pkg_path, encoding="utf-8") as fh:
        pkg = json.load(fh)
    direct = set(pkg.get("dependencies", {})) | set(pkg.get("devDependencies", {}))

    out = []
    for path, meta in lock.get("packages", {}).items():
        if not path.startswith("node_modules/"):
            continue
        name = path.split("node_modules/")[-1]
        if name not in direct:
            continue
        out.append(
            {
                "name": name,
                "version": meta.get("version", ""),
                "license": meta.get("license", "see package"),
                "kind": "npm",
                "url": f"https://www.npmjs.com/package/{name}",
            }
        )
    return sorted(out, key=lambda p: p["name"].lower())


# Licenses for the direct Rust dependencies, from each crate's own metadata.
CRATE_LICENSES = {
    "tauri": "Apache-2.0 OR MIT",
    "tauri-plugin-dialog": "Apache-2.0 OR MIT",
    "tauri-plugin-opener": "Apache-2.0 OR MIT",
    "tauri-plugin-notification": "Apache-2.0 OR MIT",
    "tauri-plugin-clipboard-manager": "Apache-2.0 OR MIT",
    "tauri-plugin-os": "Apache-2.0 OR MIT",
    "tauri-plugin-autostart": "Apache-2.0 OR MIT",
    "tauri-plugin-single-instance": "Apache-2.0 OR MIT",
    "tauri-plugin-global-shortcut": "Apache-2.0 OR MIT",
    "serde": "MIT OR Apache-2.0",
    "serde_json": "MIT OR Apache-2.0",
    "thiserror": "MIT OR Apache-2.0",
    "tokio": "MIT",
    "reqwest": "MIT OR Apache-2.0",
    "futures-util": "MIT OR Apache-2.0",
    "bytes": "MIT",
    "rusqlite": "MIT",
    "dirs": "MIT OR Apache-2.0",
    "zip": "MIT",
    "sha2": "MIT OR Apache-2.0",
    "hex": "MIT OR Apache-2.0",
    "base64": "MIT OR Apache-2.0",
    "urlencoding": "MIT",
    "once_cell": "MIT OR Apache-2.0",
    "regex": "MIT OR Apache-2.0",
    "chrono": "MIT OR Apache-2.0",
}


def cargo_packages() -> list[dict]:
    manifest = os.path.join(ROOT, "src-tauri", "Cargo.toml")
    with open(manifest, encoding="utf-8") as fh:
        text = fh.read()

    names: list[str] = []
    for block in re.split(r"^\[", text, flags=re.M):
        if not (block.startswith("dependencies]") or "dependencies]" in block.split("\n")[0]):
            continue
        if block.split("\n")[0].startswith("build-dependencies]"):
            continue
        for line in block.split("\n")[1:]:
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            name = line.split("=")[0].strip()
            if name and name not in names:
                names.append(name)

    return sorted(
        (
            {
                "name": n,
                "version": "",
                "license": CRATE_LICENSES.get(n, "see crate"),
                "kind": "cargo",
                "url": f"https://crates.io/crates/{n}",
            }
            for n in names
        ),
        key=lambda p: p["name"].lower(),
    )


def main():
    data = {
        "generatedBy": "scripts/generate_licenses.py",
        "packages": npm_packages() + cargo_packages() + FONTS + ASSETS + EXTERNAL_TOOLS,
    }
    out_dir = os.path.join(ROOT, "src-tauri", "resources")
    os.makedirs(out_dir, exist_ok=True)
    out_path = os.path.join(out_dir, "licenses.json")
    with open(out_path, "w", encoding="utf-8") as fh:
        json.dump(data, fh, indent=2, ensure_ascii=False)
    print(f"wrote {out_path} ({len(data['packages'])} entries)")


if __name__ == "__main__":
    main()
