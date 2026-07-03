#!/usr/bin/env python3
"""
LeanLayerCLI Python IPC bridge.

The bridge reads JSON Lines from stdin and writes strict JSON Lines to stdout.
It supports the Epic 1 JSON-RPC 2.0 contract and keeps the legacy generate
messages used by earlier AirLLM builds.
"""

from __future__ import annotations

import json
import sys
import urllib.request
from typing import Any


LLAMA_URL = "http://127.0.0.1:8081/v1/chat/completions"
HEALTH_URL = "http://127.0.0.1:8081/health"


def send(obj: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(obj, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def check_server() -> bool:
    try:
        urllib.request.urlopen(HEALTH_URL, timeout=2)
        return True
    except Exception:
        return False


def generate(messages: list[dict[str, Any]]) -> None:
    payload = json.dumps(
        {
            "model": "local",
            "messages": messages,
            "stream": True,
            "temperature": 0.6,
            "top_p": 0.95,
            "top_k": 20,
        }
    ).encode("utf-8")

    req = urllib.request.Request(
        LLAMA_URL,
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )

    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            for raw_line in resp:
                line = raw_line.decode("utf-8", errors="replace").strip()
                if not line.startswith("data:"):
                    continue
                data = line[5:].strip()
                if data == "[DONE]":
                    send({"type": "done"})
                    return
                try:
                    chunk = json.loads(data)
                    token = chunk["choices"][0]["delta"].get("content", "")
                except Exception:
                    token = ""
                if token:
                    send({"type": "token", "content": token})
    except Exception as exc:
        send({"type": "error", "message": str(exc)})


def rpc_success(req_id: Any, result: dict[str, Any]) -> None:
    send({"jsonrpc": "2.0", "id": req_id, "result": result})


def rpc_error(req_id: Any, code: int, message: str) -> None:
    send({"jsonrpc": "2.0", "id": req_id, "error": {"code": code, "message": message}})


def handle_jsonrpc(req: dict[str, Any]) -> None:
    req_id = req.get("id")
    method = req.get("method")
    params = req.get("params") or {}

    if method == "ping":
        rpc_success(req_id, {"status": "pong"})
    elif method == "prompt":
        rpc_success(req_id, {"status": "ack", "prompt": params.get("prompt", "")})
    elif method == "tools/list":
        rpc_success(
            req_id,
            {
                "tools": [
                    {"name": "bash", "description": "Run a sandboxed shell command"},
                    {"name": "fs_read", "description": "Read a workspace file"},
                    {"name": "fs_write", "description": "Atomically write a workspace file"},
                ]
            },
        )
    elif method == "generate":
        if not check_server():
            rpc_error(req_id, -32000, "llama-server is not running on 127.0.0.1:8081")
            return
        generate(params.get("messages", []))
        rpc_success(req_id, {"status": "done"})
    else:
        rpc_error(req_id, -32601, f"method not found: {method}")


def handle_legacy_message(msg: dict[str, Any]) -> bool:
    msg_type = msg.get("type")
    if msg_type == "generate":
        if not check_server():
            send(
                {
                    "type": "error",
                    "message": "llama-server is not running on 127.0.0.1:8081",
                }
            )
            return True
        generate(msg.get("messages", []))
        return True
    if msg_type == "ping":
        send({"type": "pong"})
        return True
    if msg_type == "quit":
        return False
    send({"type": "error", "message": f"unknown message type: {msg_type}"})
    return True


def main() -> None:
    while True:
        line = sys.stdin.readline()
        if not line:
            break

        req: dict[str, Any] | None = None
        try:
            req = json.loads(line)
            if not isinstance(req, dict):
                rpc_error(None, -32600, "request must be a JSON object")
                continue

            if req.get("jsonrpc") == "2.0":
                handle_jsonrpc(req)
                continue

            if not handle_legacy_message(req):
                break
        except Exception as exc:
            rpc_error(req.get("id") if isinstance(req, dict) else None, -32000, str(exc))


if __name__ == "__main__":
    main()
