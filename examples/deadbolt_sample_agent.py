#!/usr/bin/env python3
"""Sample agent. Admits each action through the Deadbolt sidecar. No LLM."""

import argparse
import json
import signal
import sys
import time

from deadbolt_client import admit, ensure

_stop = False


def _mark_stop(_signum, _frame):
    global _stop
    _stop = True


def _line(obj):
    json.dump(obj, sys.stdout)
    sys.stdout.write("\n")
    sys.stdout.flush()


def main(argv):
    parser = argparse.ArgumentParser(description="Deadbolt sample agent")
    parser.add_argument("--agent", required=True)
    parser.add_argument("--tool", default="shell")
    parser.add_argument("--interval", type=float, default=0.25)
    args = parser.parse_args(argv)
    signal.signal(signal.SIGTERM, _mark_stop)
    signal.signal(signal.SIGINT, _mark_stop)
    ensure(args.agent)
    while not _stop:
        decision = admit(args.agent, args.tool)
        if decision.get("decision") != "allow":
            _line({"decision": "deny", "code": decision.get("code")})
            return 2
        _line({"decision": "allow", "tool": args.tool})
        end = time.monotonic() + args.interval
        while not _stop and time.monotonic() < end:
            time.sleep(0.02)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
