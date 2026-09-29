#!/usr/bin/env python3
"""Collect a focused Roblox networking subset from a large decompile export.

Usage:
    python3 collect_roblox_decompile_subset.py /path/to/decompile
    python3 collect_roblox_decompile_subset.py /path/to/decompile -o ~/Desktop/roblox-network-focused.zip
"""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
import sys
import zipfile


# Implementation files worth including regardless of who references them.
FILENAME_RE = re.compile(
    r"(?:"
    r"RakPeer|ReliabilityLayer|SessionCrypto|RakPeerCrypto|SocketLayer|"
    r"ClientRuppGenerator|ServerRuppGenerator|(?:^|[^A-Za-z])Rupp(?:[^A-Za-z]|$)|"
    r"RbxTransport|NetworkClient|NetworkPeer|ClientReplicator|NetworkSettings|"
    r"ConnectionSettings|TeamCreate|GameJoin|(?:^|[^A-Za-z])Time(?:[^A-Za-z]|$)"
    r")",
    re.IGNORECASE,
)

# Exact symbols/configuration names used to pull in cross-file callers.
CONTENT_RE = re.compile(
    rb"(?:"
    rb"setupRuppImpl|setupRupp|getRuppHeader|SetDefaultRupp|"
    rb"processRbxOpenReply2|processRbxOpenRequest2|sendApplicationConnectionRequest|"
    rb"AssignSystemAddressToRemoteSystemList|RemoteSystemStruct|"
    rb"startOnlineBitStream|sendDatagrams|SendACKs|checkSendNak|"
    rb"encryptRakDataInPlace|decryptRakDataInPlace|SendToOrDelay|"
    rb"ClientRuppGenerator|ServerRuppGenerator|updateTokenFromDeserializationResult|"
    rb"EphemeralEarlyPubKey|RakNetEarlyPublicKey|RuppTokEn4|"
    rb"TeamCreate|team-create|ConnectionSettings|UDMUX|DataCenterId|"
    rb"RBX::Time::now|__ZN3RBX4Time3now"
    rb")",
    re.IGNORECASE,
)

ARCHIVE_OR_BINARY_SUFFIXES = {
    ".7z", ".a", ".app", ".bin", ".bmp", ".bz2", ".dmg", ".dylib",
    ".exe", ".gif", ".gz", ".ico", ".jpeg", ".jpg", ".o", ".pdf",
    ".png", ".rar", ".so", ".tar", ".tiff", ".webp", ".xz", ".zip",
}


def is_probably_text(path: Path) -> bool:
    if path.suffix.lower() in ARCHIVE_OR_BINARY_SUFFIXES:
        return False
    try:
        with path.open("rb") as source_file:
            sample = source_file.read(8192)
    except OSError:
        return False
    return b"\x00" not in sample


def content_matches(path: Path) -> bool:
    """Search in chunks while retaining overlap for boundary-spanning symbols."""
    overlap = b""
    try:
        with path.open("rb") as source_file:
            while True:
                chunk = source_file.read(1024 * 1024)
                if not chunk:
                    return False
                data = overlap + chunk
                if CONTENT_RE.search(data):
                    return True
                overlap = data[-256:]
    except OSError:
        return False


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="root directory of the extracted decompile")
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        default=Path("roblox-network-focused.zip"),
        help="output ZIP path (default: ./roblox-network-focused.zip)",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    source = args.source.expanduser().resolve()
    output = args.output.expanduser().resolve()

    if not source.is_dir():
        print(f"error: not a directory: {source}", file=sys.stderr)
        return 2
    if output == source or source in output.parents:
        print("error: place the output ZIP outside the decompile directory", file=sys.stderr)
        return 2

    selected: list[tuple[Path, Path, str]] = []
    scanned = 0

    for root, dirs, files in os.walk(source):
        root_path = Path(root)
        dirs.sort()
        files.sort()
        for name in files:
            path = root_path / name
            if path.is_symlink() or not path.is_file():
                continue
            scanned += 1
            relative_path = path.relative_to(source)

            reason = ""
            if FILENAME_RE.search(name):
                reason = "filename"
            elif len(relative_path.parts) <= 2 and re.search(
                r"version|build|manifest|readme|symbol", name, re.IGNORECASE
            ):
                reason = "top-level metadata"
            elif is_probably_text(path) and content_matches(path):
                reason = "symbol/call-site content"

            if reason:
                selected.append((path, relative_path, reason))

    output.parent.mkdir(parents=True, exist_ok=True)
    manifest_lines = [
        "Focused Roblox networking decompile subset",
        f"Source root: {source}",
        f"Files scanned: {scanned}",
        f"Files included: {len(selected)}",
        "",
        "reason\tsize\tpath",
    ]
    for path, relative_path, reason in selected:
        try:
            size = path.stat().st_size
        except OSError:
            size = -1
        manifest_lines.append(f"{reason}\t{size}\t{relative_path.as_posix()}")

    with zipfile.ZipFile(
        output,
        mode="w",
        compression=zipfile.ZIP_DEFLATED,
        compresslevel=9,
        allowZip64=True,
    ) as archive:
        archive.writestr(
            "FOCUSED-SUBSET-MANIFEST.txt", "\n".join(manifest_lines) + "\n"
        )
        for index, (path, relative_path, _reason) in enumerate(selected, 1):
            try:
                archive.write(path, relative_path.as_posix())
            except OSError as error:
                print(f"warning: skipped {relative_path}: {error}", file=sys.stderr)
            if index % 250 == 0:
                print(f"Added {index}/{len(selected)} files...")

    size_mb = output.stat().st_size / (1024 * 1024)
    print(f"Created: {output}")
    print(f"Included: {len(selected)} of {scanned} files")
    print(f"Archive size: {size_mb:.1f} MiB")
    if size_mb > 200:
        print("If needed, split it on macOS with:")
        print(f"  split -b 100m '{output}' '{output.name}.part-'")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
