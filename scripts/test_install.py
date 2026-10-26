#!/usr/bin/env python3
"""Installer contract tests using fake downloads; no network or user state writes."""
import hashlib
import os
import re
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().with_name("install.sh")


class InstallerTests(unittest.TestCase):
    def run_installer(self, mode, windows=False, source=False):
        with tempfile.TemporaryDirectory(prefix="harness installer test ") as td:
            root = Path(td)
            tools = root / "tools"
            tools.mkdir()
            payload = b'#!/bin/sh\necho "harness 1.3.0"\n'
            if mode == "invalid_binary":
                payload = b'#!/bin/sh\nexit 1\n'
            (root / "payload").write_bytes(payload)
            artifact = "harness-windows-x86_64.exe" if windows else "harness-macos-aarch64"
            digest = hashlib.sha256(payload).hexdigest()
            entries = {
                "valid": f"{digest}  {artifact}\n",
                "mismatch": f"{'0' * 64}  {artifact}\n",
                "missing_entry": f"{digest}  unrelated\n",
                "duplicate": f"{digest}  {artifact}\n" * 2,
            }
            (root / "checksums").write_text(entries.get(
                mode, entries["valid"] if mode in ("copy_failure", "invalid_binary") else ""))
            fake_curl = '''#!/bin/sh
url=""; dest=""
while [ "$#" -gt 0 ]; do
 case "$1" in -o) shift; dest="$1";; https:*) url="$1";; esac
 shift
done
case "$url" in
 */checksums.txt) [ "$TEST_MODE" != missing_manifest ] || exit 22; cp "$TEST_ROOT/checksums" "$dest";;
 *) cp "$TEST_ROOT/payload" "$dest";;
esac
'''
            (tools / "curl").write_text(fake_curl)
            (tools / "uname").write_text('#!/bin/sh\ncase "$1" in -s) echo ' +
                                        ('MINGW64_NT' if windows else 'Darwin') +
                                        ';; -m) echo arm64;; esac\n')
            if mode == "copy_failure":
                (tools / "install").write_text('''#!/bin/sh
for destination do :; done
printf partial > "$destination"
exit 1
''')
            if source:
                (tools / "git").write_text('''#!/bin/sh
for destination do :; done
mkdir -p "$destination"
''')
                (tools / "cargo").write_text('''#!/bin/sh
mkdir -p target/release-lto
cp "$TEST_ROOT/payload" "target/release-lto/$TEST_BINARY"
''')
            for p in tools.iterdir():
                p.chmod(0o755)
            destination = root / "install"
            destination.mkdir()
            target = destination / ("harness.exe" if windows else "harness")
            target.write_text("previous installation")
            env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                       HOME=td, HARNESS_INSTALL_DIR=str(destination), TEST_ROOT=td, TEST_MODE=mode,
                       TEST_BINARY=target.name, HARNESS_INSTALL_SOURCE="1" if source else "0")
            result = subprocess.run(["bash", str(SCRIPT), "v1.3.0"], env=env, cwd=root,
                                    capture_output=True, text=True, timeout=15)
            contents = target.read_bytes()
            self.assertFalse((root / ".harness/config.toml").exists())
            self.assertEqual(list(destination.glob(".harness-install.*")), [])
            if mode == "valid":
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(contents, payload)
            else:
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(contents, b"previous installation")

    def test_verified_install(self):
        self.run_installer("valid")

    def test_windows_git_bash_filename(self):
        self.run_installer("valid", windows=True)

    def test_source_install(self):
        self.run_installer("valid", source=True)

    def test_partial_copy_preserves_previous_installation(self):
        for source in (False, True):
            with self.subTest(source=source):
                self.run_installer("copy_failure", source=source)

    def test_unusable_candidate_preserves_previous_installation(self):
        for source in (False, True):
            with self.subTest(source=source):
                self.run_installer("invalid_binary", source=source)

    def test_rejects_unverified_binary_without_overwriting(self):
        for mode in ("mismatch", "missing_entry", "duplicate", "missing_manifest"):
            with self.subTest(mode=mode):
                self.run_installer(mode)


class HomebrewTests(unittest.TestCase):
    def test_refresh_preserves_all_checksum_lines_and_fails_atomically(self):
        with tempfile.TemporaryDirectory(prefix="harness-formula-test-") as td:
            root = Path(td)
            tools = root / "tools"
            tools.mkdir()
            artifacts = ["harness-macos-aarch64", "harness-macos-x86_64",
                         "harness-linux-aarch64", "harness-linux-x86_64",
                         "LICENSE.txt", "THIRD-PARTY-LICENSES.html"]
            (root / "assets").mkdir()
            digests = {}
            for artifact in artifacts:
                payload = ("Synthetic fixture " + artifact).encode()
                (root / "assets" / artifact).write_bytes(payload)
                digests[artifact] = hashlib.sha256(payload).hexdigest()
            (root / "checksums.txt").write_text("".join(f"{digests[a]}  {a}\n" for a in artifacts))
            curl = tools / "curl"
            curl.write_text('''#!/bin/sh
url=""; dest=""
while [ "$#" -gt 0 ]; do
 case "$1" in -o) shift; dest="$1";; https:*) url="$1";; esac
 shift
done
case "$url" in
 */checksums.txt) cp "$TEST_ROOT/checksums.txt" "$dest";;
 *) cp "$TEST_ROOT/assets/${url##*/}" "$dest";;
esac
''')
            curl.chmod(0o755)
            formula = root / "harness.rb"
            original = SCRIPT.parent.parent.joinpath("homebrew/harness.rb").read_text()
            formula.write_text(original)
            env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                       TEST_ROOT=td, HARNESS_HOMEBREW_FORMULA=str(formula))
            command = ["bash", str(SCRIPT.with_name("update-homebrew-sha.sh")), "v1.3.1"]
            result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            updated = formula.read_text()
            for artifact in artifacts:
                pattern = r'url "[^"\n]*/' + re.escape(artifact) + r'"\s*\n\s*sha256 "([^"]+)"'
                self.assertEqual(re.findall(pattern, updated), [digests[artifact]])
            self.assertIn('version "1.3.1"', updated)
            (root / "assets" / "THIRD-PARTY-LICENSES.html").write_bytes(b"tampered license")
            result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=15)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(formula.read_text(), updated)


if __name__ == "__main__":
    unittest.main()
