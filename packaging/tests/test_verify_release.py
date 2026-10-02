"""Tiny synthetic releases exercise the same verifier used before draft creation."""

import importlib.util
import io
import json
import os
from pathlib import Path
import plistlib
import struct
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile


SPEC = importlib.util.spec_from_file_location("verify_release", Path(__file__).parents[1] / "verify_release.py")
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)
TAG = "v1.2.3"


def executable(target):
    data = bytearray(256)
    if "linux" in target:
        data[:6] = b"\x7fELF\x02\x01"
        struct.pack_into("<H", data, 18, 183 if target.startswith("aarch64") else 62)
    else:
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 60, 128)
        data[128:132] = b"PE\0\0"
        struct.pack_into("<H", data, 132, 0xAA64 if target.startswith("aarch64") else 0x8664)
    return bytes(data)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        environment = patch.dict(os.environ, {"GITHUB_REPOSITORY": "owner/repo", "GITHUB_RUN_ID": "12345", "GITHUB_RUN_ATTEMPT": "2", "GITHUB_SERVER_URL": "https://github.com"})
        environment.start()
        self.addCleanup(environment.stop)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "source"
        self.dist = Path(self.temp.name) / "dist"
        self.root.mkdir()
        self.dist.mkdir()
        fixtures = {
            "Cargo.toml": '[package]\nname = "oxidify"\nversion = "1.2.3"\n',
            "Cargo.lock": '[[package]]\nname = "oxidify"\nversion = "1.2.3"\n',
            "packaging/release-notes/v1.2.3.md": "Release notes\n",
            "packaging/applications/oxidify.desktop": "[Desktop Entry]\n",
            "packaging/macos/bundle.sh": "macOS only\n",
            "docs/_config.yml": 'oxidify_version: "1.2.3"\n',
            "docs/_data/versions.yml": "current: v1.2.3\n",
        }
        fixtures.update({p: f"{p} license or documentation\n" for p in release.DOCUMENTS.values()})
        for name, contents in fixtures.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(contents, encoding="utf-8")
        self.git("init", "-q")
        self.commit()
        self.sha = self.git("rev-parse", "HEAD")

    def git(self, *args):
        return release.git(self.root, *args)

    def commit(self):
        self.git("add", ".")
        self.git("-c", "user.name=Release test", "-c", "user.email=release@example.invalid", "commit", "-qm", "Fixture")

    def archive(self, target, changes=None, mode=0o755, duplicate=None, symlink=None):
        inventory = release.expected_inventory(self.root, target)
        members = {name: (self.root / original).read_bytes() if original else executable(target)
                   for name, original in inventory.items()}
        for name, contents in (changes or {}).items():
            if contents is None:
                members.pop(name, None)
            else:
                members[name] = contents
        prefix = f"oxidify-{TAG}-{target}"
        path = self.dist / (prefix + (".zip" if "windows" in target else ".tar.gz"))
        if "windows" in target:
            with zipfile.ZipFile(path, "w") as archive:
                for name, contents in members.items():
                    archive.writestr(f"{prefix}/{name}", contents)
                if symlink:
                    info = zipfile.ZipInfo(f"{prefix}/{symlink}")
                    info.create_system = 3
                    info.external_attr = 0o120777 << 16
                    archive.writestr(info, "LICENSE")
        else:
            with tarfile.open(path, "w:gz") as archive:
                for name, contents in members.items():
                    info = tarfile.TarInfo(f"{prefix}/{name}")
                    info.size = len(contents)
                    info.mode = mode
                    archive.addfile(info, io.BytesIO(contents))
                if duplicate:
                    info = tarfile.TarInfo(f"{prefix}/{duplicate}")
                    info.size = len(members[duplicate])
                    archive.addfile(info, io.BytesIO(members[duplicate]))
                if symlink:
                    info = tarfile.TarInfo(f"{prefix}/{symlink}")
                    info.type = tarfile.SYMTYPE
                    info.linkname = "LICENSE"
                    archive.addfile(info)
        return path

    def complete_release(self):
        for target in release.TARGETS:
            if target == "macos-universal":
                name = next(iter(release.artifact_names(TAG, target)))
                (self.dist / name).write_bytes(b"DMG" + b"koly" + bytes(508))
            else:
                self.archive(target)
                if "windows" in target:
                    name = next(n for n in release.artifact_names(TAG, target) if n.endswith(".exe"))
                    (self.dist / name).write_bytes(b"MZinstaller")
            # Do not need a Rust installation to exercise metadata serialization.
            with patch.object(subprocess, "check_output", wraps=subprocess.check_output) as output:
                output.side_effect = lambda command, **kwargs: "rustc 1.98.0 (fixture)\n" if command[0] == "rustc" else output._mock_wraps(command, **kwargs)
                release.write_report(self.root, self.dist, TAG, self.sha, target)

    def test_version_and_sha_match(self):
        self.assertEqual(release.source(self.root, TAG, self.sha), self.sha)

    def test_source_mismatch(self):
        with self.assertRaisesRegex(ValueError, "validated source SHA"):
            release.source(self.root, TAG, "0" * 40)

    def test_dirty_source_rejected(self):
        (self.root / "LICENSE").write_text("changed")
        with self.assertRaisesRegex(ValueError, "dirty"):
            release.source(self.root, TAG, self.sha)

    def test_invalid_tag_rejected(self):
        for tag in ("main", "v1.2.3-rc1", "v1.2.3\n", "v1.2.4", "../v1.2.3"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                release.source(self.root, tag)

    def test_release_input_mismatches(self):
        cases = {
            "Cargo.lock": '[[package]]\nname="oxidify"\nversion="9.9.9"\n',
            "docs/_config.yml": 'oxidify_version: "9.9.9"\n',
            "docs/_data/versions.yml": "current: v9.9.9\n",
            "packaging/release-notes/v1.2.3.md": " \n",
        }
        for name, bad in cases.items():
            path = self.root / name
            original = path.read_text()
            path.write_text(bad)
            with self.subTest(name=name), self.assertRaises(ValueError):
                release.source(self.root, TAG)
            path.write_text(original)

    def test_archive_platforms(self):
        for target in release.TARGETS[:-1]:
            with self.subTest(target=target):
                release.verify_archive(self.root, self.archive(target), TAG, target)

    def test_archive_missing_extra_empty_and_changed_files(self):
        for target in (release.TARGETS[0], release.TARGETS[2]):
            for changes in ({"LICENSE": None}, {"unexpected": b"bad"}, {"NOTICE": b""}, {"yt-dlp-LICENSE.txt": b"wrong"}):
                with self.subTest(target=target, changes=changes), self.assertRaises(ValueError):
                    release.verify_archive(self.root, self.archive(target, changes), TAG, target)

    def test_wrong_architecture_rejected(self):
        for target, other in ((release.TARGETS[0], release.TARGETS[1]), (release.TARGETS[2], release.TARGETS[3])):
            name = "oxidify.exe" if "windows" in target else "oxidify"
            path = self.archive(target, {name: executable(other)})
            with self.subTest(target=target), self.assertRaisesRegex(ValueError, "architecture differs"):
                release.verify_archive(self.root, path, TAG, target)

    def test_linux_executable_permission_required(self):
        target = release.TARGETS[0]
        with self.assertRaisesRegex(ValueError, "execute permission"):
            release.verify_archive(self.root, self.archive(target, mode=0o644), TAG, target)

    def test_symlinks_and_duplicate_members_rejected(self):
        for target in (release.TARGETS[0], release.TARGETS[2]):
            with self.subTest(target=target), self.assertRaises(ValueError):
                release.verify_archive(self.root, self.archive(target, symlink="link"), TAG, target)
        target = release.TARGETS[0]
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            release.verify_archive(self.root, self.archive(target, duplicate="LICENSE"), TAG, target)

    def test_unsafe_member_paths_rejected(self):
        for name in ("/absolute", "../escape", "a/../../escape", "a\\escape", "a//b", "./a"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                release.check_member_name(name)

    def test_windows_document_line_endings(self):
        target = release.TARGETS[2]
        license_path = self.root / "LICENSE"
        # write_text uses the host's line endings. Normalize the fixture once
        # before constructing either form, so Windows cannot produce CRCRLF.
        lf = license_path.read_bytes().replace(b"\r\n", b"\n")
        for source in (lf, lf.replace(b"\n", b"\r\n")):
            license_path.write_bytes(source)
            for contents in (lf, lf.replace(b"\n", b"\r\n")):
                with self.subTest(source=source, archive=contents):
                    release.verify_archive(self.root, self.archive(target, {"LICENSE": contents}), TAG, target)
        # Only normal LF/CRLF differences are harmless, not malformed text.
        with self.assertRaisesRegex(ValueError, "document differs"):
            release.verify_archive(self.root, self.archive(target, {"LICENSE": lf.replace(b"\n", b"\r\r\n")}), TAG, target)

    def test_complete_release_and_rerun(self):
        self.complete_release()
        release.verify_release(self.root, self.dist, TAG, self.sha)
        checksums = (self.dist / "checksums.txt").read_bytes()
        self.assertEqual(len(checksums.splitlines()), 8)
        self.assertEqual(json.loads((self.dist / "provenance.json").read_text())["source_sha"], self.sha)
        release.verify_release(self.root, self.dist, TAG, self.sha)
        self.assertEqual((self.dist / "checksums.txt").read_bytes(), checksums)

    def test_missing_and_extra_artifacts_rejected(self):
        self.complete_release()
        extra = self.dist / "unrelated.txt"
        extra.write_text("extra")
        with self.assertRaisesRegex(ValueError, "Release files differ"):
            release.verify_release(self.root, self.dist, TAG, self.sha)
        extra.unlink()
        next(self.dist.glob("*.dmg")).unlink()
        with self.assertRaisesRegex(ValueError, "Release files differ"):
            release.verify_release(self.root, self.dist, TAG, self.sha)

    def test_build_report_source_and_hash_mismatch(self):
        self.complete_release()
        path = self.dist / f"{release.TARGETS[0]}.build.json"
        original = json.loads(path.read_text())
        for field, value in (("source_sha", "0" * 40), ("tag", "v9.9.9"), ("target", "other"), ("rustc", ""), ("artifacts", {}), ("workflow_run", {})):
            path.write_text(json.dumps({**original, field: value}))
            with self.subTest(field=field), self.assertRaises(ValueError):
                release.verify_release(self.root, self.dist, TAG, self.sha)

    def test_earlier_attempt_accepted_but_other_run_rejected(self):
        self.complete_release()
        path = self.dist / f"{release.TARGETS[0]}.build.json"
        report = json.loads(path.read_text())
        report["workflow_run"]["run_attempt"] = 1
        path.write_text(json.dumps(report))
        release.verify_release(self.root, self.dist, TAG, self.sha)
        for key, value in (("run_attempt", 3), ("run_id", "other")):
            original = report["workflow_run"][key]
            report["workflow_run"][key] = value
            path.write_text(json.dumps(report))
            with self.subTest(key=key), self.assertRaises(ValueError):
                release.verify_release(self.root, self.dist, TAG, self.sha)
            report["workflow_run"][key] = original

    def test_corrupted_download_rejected(self):
        self.complete_release()
        path = next(self.dist.glob("*-setup.exe"))
        path.write_bytes(b"MZchanged installer")
        with self.assertRaisesRegex(ValueError, "artifact hashes differ"):
            release.verify_release(self.root, self.dist, TAG, self.sha)

    def test_bad_installer_or_dmg_rejected(self):
        self.complete_release()
        for suffix in ("*-setup.exe", "*.dmg"):
            path = next(self.dist.glob(suffix))
            original = path.read_bytes()
            path.write_bytes(b"bad" * 300)
            with self.subTest(suffix=suffix), self.assertRaises(ValueError):
                release.verify_release(self.root, self.dist, TAG, self.sha)
            path.write_bytes(original)

    def test_checksum_missing_duplicate_extra_malformed_and_changed(self):
        path = self.dist / "artifact"
        path.write_bytes(b"artifact")
        line = f"{release.sha256(path)}  artifact\n"
        for contents in ("", line * 2, line + "0" * 64 + "  other\n", "malformed\n", "0" * 64 + "  artifact\n"):
            (self.dist / "checksums.txt").write_text(contents)
            with self.subTest(contents=contents), self.assertRaises(ValueError):
                release.verify_checksums(self.dist, {"artifact"})
        (self.dist / "checksums.txt").write_text(line)
        release.verify_checksums(self.dist, {"artifact"})

    def test_macos_bundle_version_and_licenses(self):
        app = self.dist / "Oxidify.app"
        resources = app / "Contents/Resources"
        resources.mkdir(parents=True)
        for name, original in release.DOCUMENTS.items():
            if name != "README.md":
                (resources / name).write_bytes((self.root / original).read_bytes())
        (resources / "oxidify.icns").write_bytes(b"icon")
        info = app / "Contents/Info.plist"
        info.write_bytes(plistlib.dumps({"CFBundleShortVersionString": "1.2.3", "CFBundleVersion": "1.2.3", "CFBundleExecutable": "oxidify"}))
        release.macos_bundle(self.root, app, TAG)
        with self.assertRaisesRegex(ValueError, "version differs"):
            release.macos_bundle(self.root, app, "v9.9.9")
        (resources / "LICENSE").write_bytes(b"wrong")
        with self.assertRaisesRegex(ValueError, "document differs"):
            release.macos_bundle(self.root, app, TAG)

    def test_installed_windows_files(self):
        for name, original in release.DOCUMENTS.items():
            (self.dist / name).write_bytes((self.root / original).read_bytes())
        (self.dist / "oxidify.exe").write_bytes(executable(release.TARGETS[2]))
        release.installed(self.root, self.dist, release.TARGETS[2])
        (self.dist / "yt-dlp-LICENSE.txt").write_bytes(b"wrong")
        with self.assertRaisesRegex(ValueError, "document differs"):
            release.installed(self.root, self.dist, release.TARGETS[2])


if __name__ == "__main__":
    unittest.main()
