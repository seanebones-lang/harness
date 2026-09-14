#!/usr/bin/env python3
"""Offline CLI smoke in disposable user/project state; never uses real credentials."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    binary = str(Path(sys.argv[1]).resolve(strict=True))
    with tempfile.TemporaryDirectory(prefix="harness-smoke-") as scratch:
        root = Path(scratch)
        # Only this child process sees the temporary home. The caller is unchanged.
        env = {k: v for k, v in os.environ.items() if k in
               ("PATH", "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT", "LANG", "TERM")}
        env.update(HOME=scratch, USERPROFILE=scratch, HARNESS_SKIP_LSP="1")
        (root / ".harness").mkdir()
        config = root / ".harness/config.toml"
        config.write_text('[memory]\nenabled = false\n[ambient]\nenabled = false\n')
        def run(*args):
            result = subprocess.run([binary, *args], cwd=root, env=env, stdin=subprocess.DEVNULL,
                                    capture_output=True, text=True, timeout=30)
            if result.returncode:
                raise RuntimeError(f"{args}: exit {result.returncode}\n{result.stderr}\n{result.stdout}")
            print(f"PASS {' '.join(args)}")
            return result.stdout
        version = run("--version").strip()
        assert version.startswith("harness "), version
        run("--help")
        run("doctor")
        run("sessions")
        run("setup", "--help")
        run("swarm", "list")
        run("swarm", "gc", "--dry-run")
        run("mcp", "roots")
        run("route", "set", "openai:smoke-primary", "ollama:smoke-fallback")
        route = run("route", "show")
        assert "smoke-primary" in route and "smoke-fallback" in route, route
        run("route", "move", "ollama", "1")
        print(json.dumps({"binary": binary, "version": version, "offline_smoke": "passed"}))
        print("Live provider, TUI, browser and platform package acceptance remain separate gates.")


if __name__ == "__main__":
    main()
