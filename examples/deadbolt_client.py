#!/usr/bin/env python3
"""Bolt-on client for `harness deadbolt serve`. Stdlib only. Not a model tool."""

import argparse
import http.client
import json
import os
import socket
import sys


def sock_path():
    raw = os.environ.get("DEADBOLT_SOCK")
    if raw:
        return raw
    return os.path.join(os.path.expanduser("~"), ".harness", "deadbolt.sock")


class _UnixHTTPConnection(http.client.HTTPConnection):
    def __init__(self, path):
        super().__init__("localhost")
        self._path = path

    def connect(self):
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(self._path)
        self.sock = sock


def _call(method, path, body=None):
    conn = _UnixHTTPConnection(sock_path())
    payload = None if body is None else json.dumps(body).encode("utf-8")
    headers = {"Content-Type": "application/json", "Connection": "close"}
    token = os.environ.get("DEADBOLT_TOKEN")
    if token:
        headers["X-Deadbolt-Token"] = token
    conn.request(method, path, body=payload, headers=headers)
    resp = conn.getresponse()
    raw = resp.read()
    conn.close()
    if not raw:
        raise SystemExit("deadbolt:empty")
    return json.loads(raw.decode("utf-8"))


def ensure(agent_id):
    return _call("POST", "/ensure", {"agent_id": agent_id})


def admit(agent_id, tool):
    return _call("POST", "/admit", {"agent_id": agent_id, "tool": tool})


def register_child(parent, child, swarm_task_id=None):
    body = {"parent": parent, "child": child}
    if swarm_task_id:
        body["swarm_task_id"] = swarm_task_id
    return _call("POST", "/register_child", body)


def status(agent_id=None):
    path = "/status" if not agent_id else "/status?agent=" + agent_id
    return _call("GET", path)


def _print(obj):
    json.dump(obj, sys.stdout)
    sys.stdout.write("\n")


def main(argv):
    parser = argparse.ArgumentParser(description="Deadbolt Unix-socket client")
    sub = parser.add_subparsers(dest="cmd", required=True)
    p_ensure = sub.add_parser("ensure")
    p_ensure.add_argument("--agent", required=True)
    p_admit = sub.add_parser("admit")
    p_admit.add_argument("--agent", required=True)
    p_admit.add_argument("--tool", required=True)
    p_reg = sub.add_parser("register-child")
    p_reg.add_argument("--parent", required=True)
    p_reg.add_argument("--child", required=True)
    p_reg.add_argument("--swarm-task")
    p_status = sub.add_parser("status")
    p_status.add_argument("--agent")
    args = parser.parse_args(argv)
    if args.cmd == "ensure":
        _print(ensure(args.agent))
    elif args.cmd == "admit":
        _print(admit(args.agent, args.tool))
    elif args.cmd == "register-child":
        _print(register_child(args.parent, args.child, args.swarm_task))
    elif args.cmd == "status":
        _print(status(args.agent))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
