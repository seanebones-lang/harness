#!/usr/bin/env python3
"""Installer contract tests using fake downloads; no network or user state writes."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().with_name("install.sh")


class InstallerTests(unittest.TestCase):
    def run_installer(self, mode, windows=False):
        with tempfile.TemporaryDirectory(prefix="harness-installer-test-") as td:
            root = Path(td)
            tools = root / "tools"
            tools.mkdir()
            payload = b'#!/bin/sh\necho "harness 1.3.0"\n'
            (root / "payload").write_bytes(payload)
            artifact = "harness-windows-x86_64.exe" if windows else "harness-macos-aarch64"
            digest = hashlib.sha256(payload).hexdigest()
            entries = {
                "valid": f"{digest}  {artifact}\n",
                "mismatch": f"{'0' * 64}  {artifact}\n",
                "missing_entry": f"{digest}  unrelated\n",
                "duplicate": f"{digest}  {artifact}\n" * 2,
            }
            (root / "checksums").write_text(entries.get(mode, ""))
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
            for p in tools.iterdir():
                p.chmod(0o755)
            destination = root / "install"
            destination.mkdir()
            target = destination / ("harness.exe" if windows else "harness")
            target.write_text("previous installation")
            env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                       HOME=td, HARNESS_INSTALL_DIR=str(destination), TEST_ROOT=td, TEST_MODE=mode)
            result = subprocess.run(["bash", str(SCRIPT), "v1.3.0"], env=env, cwd=root,
                                    capture_output=True, text=True, timeout=15)
            contents = target.read_bytes()
            self.assertFalse((root / ".harness/config.toml").exists())
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
            payload = b"synthetic release binary"
            (root / "payload").write_bytes(payload)
            digest = hashlib.sha256(payload).hexdigest()
            artifacts = ["harness-macos-aarch64", "harness-macos-x86_64",
                         "harness-linux-aarch64", "harness-linux-x86_64"]
            (root / "checksums.txt").write_text("".join(f"{digest}  {a}\n" for a in artifacts))
            curl = tools / "curl"
            curl.write_text('''#!/bin/sh
url=""; dest=""
while [ "$#" -gt 0 ]; do
 case "$1" in -o) shift; dest="$1";; https:*) url="$1";; esac
 shift
done
case "$url" in
 */checksums.txt) cp "$TEST_ROOT/checksums.txt" "$dest";;
 *) cp "$TEST_ROOT/payload" "$dest";;
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
            self.assertEqual(updated.count('sha256 "' + digest + '"'), 4)
            self.assertIn('version "1.3.1"', updated)
            (root / "payload").write_bytes(b"tampered")
            result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=15)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(formula.read_text(), updated)


if __name__ == "__main__":
    unittest.main()
