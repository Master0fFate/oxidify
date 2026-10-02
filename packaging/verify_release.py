#!/usr/bin/env python3
"""Validate release inputs and artifacts using only Python 3.11+'s stdlib.

Build reports bind each runner's output hashes to the checked source commit.
They are audit metadata, not signed attestations or a bit-reproducibility claim.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import plistlib
import re
import stat
import struct
import subprocess
import tarfile
import tomllib
import zipfile


TARGETS = (
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "macos-universal",
)
DOCUMENTS = {
    "README.md": "README.md",
    "LICENSE": "LICENSE",
    "NOTICE": "NOTICE",
    "yt-dlp-NOTICE.txt": "third_party/yt-dlp/NOTICE",
    "yt-dlp-LICENSE.txt": "third_party/yt-dlp/LICENSE",
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).rstrip("\r\n")


def source(root, tag, expected_sha=None):
    require(re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", tag), "Expected a stable vX.Y.Z tag")
    version = tag[1:]
    manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    require(manifest["package"]["version"] == version, "Tag and Cargo.toml disagree")
    packages = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))["package"]
    require([p["version"] for p in packages if p["name"] == "oxidify"] == [version],
            "Tag and Cargo.lock disagree")
    notes = root / f"packaging/release-notes/{tag}.md"
    require(notes.is_file() and notes.read_text(encoding="utf-8").strip(), "Release notes are missing")
    require(f'oxidify_version: "{version}"' in (root / "docs/_config.yml").read_text().splitlines(),
            "Download page version differs")
    require(f"current: {tag}" in (root / "docs/_data/versions.yml").read_text().splitlines(),
            "Version dropdown differs")
    sha = git(root, "rev-parse", "HEAD")
    require(re.fullmatch(r"[0-9a-f]{40}", sha), "Expected a full source commit SHA")
    if expected_sha is not None:
        require(sha == expected_sha, "Checkout differs from the validated source SHA")
    require(not git(root, "status", "--porcelain", "--untracked-files=no"), "Tracked source is dirty")
    return sha


def artifact_names(tag, target):
    require(target in TARGETS, f"Unsupported release target: {target}")
    prefix = f"oxidify-{tag}-{target}"
    if target == "macos-universal":
        return {prefix + ".dmg"}
    if "windows" in target:
        return {prefix + ".zip", prefix + "-setup.exe"}
    return {prefix + ".tar.gz"}


def sha256(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def same_document(actual, expected, label):
    # Git on Windows can check out CRLF. Licensing text must still be identical.
    require(actual.replace(b"\r\n", b"\n") == expected.replace(b"\r\n", b"\n"),
            f"Packaged document differs from source: {label}")


def binary_header(data, target):
    if "linux" in target:
        require(len(data) >= 20 and data[:6] == b"\x7fELF\x02\x01", "Expected a 64-bit little-endian ELF")
        machine = struct.unpack_from("<H", data, 18)[0]
        require(machine == (183 if target.startswith("aarch64") else 62), "ELF architecture differs")
    else:
        require(len(data) >= 64 and data[:2] == b"MZ", "Expected a PE executable")
        offset = struct.unpack_from("<I", data, 60)[0]
        require(offset + 6 <= len(data) and data[offset:offset + 4] == b"PE\0\0", "Invalid PE header")
        machine = struct.unpack_from("<H", data, offset + 4)[0]
        require(machine == (0xAA64 if target.startswith("aarch64") else 0x8664), "PE architecture differs")


def expected_inventory(root, target):
    expected = dict(DOCUMENTS)
    expected["oxidify.exe" if "windows" in target else "oxidify"] = None
    if "linux" in target:
        paths = git(root, "ls-files", "-z", "--", "packaging").split("\0")
        expected.update({p: p for p in paths if p and not p.startswith("packaging/macos/")})
    return expected


def check_member_name(name):
    path = PurePosixPath(name)
    require(not path.is_absolute() and ".." not in path.parts and "\\" not in name,
            f"Unsafe archive path: {name}")
    require(str(path) == name.rstrip("/"), f"Noncanonical archive path: {name}")
    return str(path)


def verify_archive(root, archive, tag, target):
    prefix = f"oxidify-{tag}-{target}"
    expected = {f"{prefix}/{name}": original for name, original in expected_inventory(root, target).items()}
    seen = set()
    files = set()

    def visit(name, is_dir, size, mode, read):
        name = check_member_name(name)
        require(name not in seen, f"Duplicate archive member: {name}")
        seen.add(name)
        if is_dir:
            require(any(p.startswith(name + "/") for p in expected), f"Unexpected archive directory: {name}")
            return
        require(name in expected, f"Unexpected archive member: {name}")
        require(size > 0, f"Empty archive member: {name}")
        files.add(name)
        original = expected[name]
        if original is None:
            binary_header(read(4096), target)
            if "linux" in target:
                require(mode & 0o111, "Linux executable has no execute permission")
        else:
            original_bytes = (root / original).read_bytes()
            require(size <= len(original_bytes) * 2 + 1, f"Oversized archive document: {name}")
            same_document(read(), original_bytes, name)

    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive) as zipped:
            require(zipped.testzip() is None, "Corrupt ZIP member")
            for info in zipped.infolist():
                mode = info.external_attr >> 16
                require(not stat.S_ISLNK(mode), f"Archive symlink: {info.filename}")
                with zipped.open(info) as file:
                    visit(info.filename, info.is_dir(), info.file_size, mode, file.read)
    else:
        with tarfile.open(archive, "r:gz") as tar:
            for info in tar:
                require(info.isfile() or info.isdir(), f"Archive special file: {info.name}")
                if info.isdir():
                    visit(info.name, True, 0, info.mode, None)
                else:
                    with tar.extractfile(info) as file:
                        visit(info.name, False, info.size, info.mode, file.read)
    require(files == set(expected), f"Missing archive files: {sorted(set(expected) - files)}")


def verify_payload(root, directory, tag, target):
    for name in artifact_names(tag, target):
        path = directory / name
        require(path.is_file() and not path.is_symlink() and path.stat().st_size, f"Missing or empty artifact: {name}")
        if name.endswith((".zip", ".tar.gz")):
            verify_archive(root, path, tag, target)
        elif name.endswith(".exe"):
            # Inno's bootstrapper architecture is independent of the payload.
            with path.open("rb") as file:
                require(file.read(2) == b"MZ", f"Invalid installer header: {name}")
        else:
            with path.open("rb") as file:
                file.seek(-512, os.SEEK_END)
                require(file.read(4) == b"koly", f"Invalid UDIF disk image trailer: {name}")


def installed(root, directory, target):
    for name, original in DOCUMENTS.items():
        same_document((directory / name).read_bytes(), (root / original).read_bytes(), name)
    with (directory / "oxidify.exe").open("rb") as file:
        binary_header(file.read(4096), target)


def macos_bundle(root, app, tag):
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    require(info.get("CFBundleShortVersionString") == tag[1:] and info.get("CFBundleVersion") == tag[1:],
            "App bundle version differs")
    require(info.get("CFBundleExecutable") == "oxidify", "App bundle executable differs")
    for name, original in DOCUMENTS.items():
        if name != "README.md":
            same_document((app / "Contents/Resources" / name).read_bytes(), (root / original).read_bytes(), name)
    require((app / "Contents/Resources/oxidify.icns").stat().st_size, "App icon is empty")


def workflow_run():
    repository = os.environ.get("GITHUB_REPOSITORY", "")
    run_id = os.environ.get("GITHUB_RUN_ID", "")
    attempt = os.environ.get("GITHUB_RUN_ATTEMPT", "")
    require(re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository), "Missing workflow repository")
    require(run_id.isdecimal() and attempt.isdecimal() and int(attempt) > 0, "Missing workflow run identity")
    return {"repository": repository, "run_id": run_id, "run_attempt": int(attempt),
            "url": f"{os.environ.get('GITHUB_SERVER_URL', 'https://github.com')}/{repository}/actions/runs/{run_id}"}


def write_report(root, directory, tag, sha, target):
    source(root, tag, sha)
    verify_payload(root, directory, tag, target)
    report = {
        "source_sha": sha, "tag": tag, "target": target, "workflow_run": workflow_run(),
        "rustc": subprocess.check_output(["rustc", "-Vv"], text=True).strip(),
        "artifacts": {name: sha256(directory / name) for name in sorted(artifact_names(tag, target))},
    }
    (directory / f"{target}.build.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


def verify_release(root, directory, tag, sha):
    source(root, tag, sha)
    artifacts = set().union(*(artifact_names(tag, target) for target in TARGETS))
    reports = {f"{target}.build.json" for target in TARGETS}
    generated = {"checksums.txt", "provenance.json"}
    actual = {p.name for p in directory.iterdir()}
    require(actual - generated == artifacts | reports, f"Release files differ: {sorted((actual - generated) ^ (artifacts | reports))}")
    require(all(p.is_file() and not p.is_symlink() and p.stat().st_size for p in directory.iterdir()),
            "Release directory contains non-regular or empty files")
    builds = []
    current_run = workflow_run()
    for target in TARGETS:
        verify_payload(root, directory, tag, target)
        report = json.loads((directory / f"{target}.build.json").read_text(encoding="utf-8"))
        require(report.get("source_sha") == sha and report.get("tag") == tag and report.get("target") == target,
                f"Build source/tag/target differs: {target}")
        require(isinstance(report.get("rustc"), str) and report["rustc"].startswith("rustc "), f"Missing compiler provenance: {target}")
        recorded_run = report.get("workflow_run", {})
        require(all(recorded_run.get(key) == current_run[key] for key in ("repository", "run_id", "url")),
                f"Build workflow run differs: {target}")
        attempt = recorded_run.get("run_attempt")
        # Rerunning failed jobs legitimately retains successful earlier artifacts.
        require(type(attempt) is int and 0 < attempt <= current_run["run_attempt"],
                f"Build workflow attempt differs: {target}")
        hashes = {name: sha256(directory / name) for name in sorted(artifact_names(tag, target))}
        require(report.get("artifacts") == hashes, f"Build artifact hashes differ: {target}")
        builds.append(report)
    provenance = {"schema_version": 1, "source_sha": sha, "tag": tag, "builds": builds}
    (directory / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n", encoding="utf-8")
    checksum_names = sorted(artifacts | {"provenance.json"})
    (directory / "checksums.txt").write_text("".join(f"{sha256(directory / name)}  {name}\n" for name in checksum_names), encoding="utf-8")
    verify_checksums(directory, set(checksum_names))


def verify_checksums(directory, expected):
    seen = set()
    for line in (directory / "checksums.txt").read_text(encoding="utf-8").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([^/\\]+)", line)
        require(match is not None, "Malformed checksum entry")
        digest, name = match.groups()
        require(name in expected and name not in seen, f"Unexpected or duplicate checksum: {name}")
        require(sha256(directory / name) == digest, f"Checksum mismatch: {name}")
        seen.add(name)
    require(seen == expected, f"Missing checksums: {sorted(expected - seen)}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("source", "report", "release", "installed", "macos-bundle"))
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--tag", required=True)
    parser.add_argument("--sha")
    parser.add_argument("--directory", type=Path, default=Path("dist"))
    parser.add_argument("--target", choices=TARGETS)
    args = parser.parse_args()
    if args.command == "source":
        print(source(args.root, args.tag, args.sha))
    elif args.command == "installed":
        require(args.target is not None and "windows" in args.target, "Installed validation needs a Windows target")
        installed(args.root, args.directory, args.target)
    elif args.command == "macos-bundle":
        macos_bundle(args.root, args.directory, args.tag)
    else:
        require(args.sha is not None, "Artifact validation requires the checked source SHA")
        if args.command == "report":
            write_report(args.root, args.directory, args.tag, args.sha, args.target)
        else:
            verify_release(args.root, args.directory, args.tag, args.sha)


if __name__ == "__main__":
    main()
