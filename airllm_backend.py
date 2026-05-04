#!/usr/bin/env python3
"""
airllm_backend.py — llama.cpp backend wrapper.
Talks to llama-server's OpenAI-compatible API and streams tokens to stdout as JSON-lines.
"""

import sys
import json
import urllib.request

LLAMA_URL = "http://127.0.0.1:8081/v1/chat/completions"

def send(obj: dict):
    print(json.dumps(obj), flush=True)

def log(msg: str):
    sys.stderr.write(f"[backend] {msg}\n")
    sys.stderr.flush()

def check_server():
    try:
        urllib.request.urlopen("http://127.0.0.1:8081/health")
        return True
    except:
        return False

def generate(messages: list):
    payload = json.dumps({
        "model": "local",
        "messages": messages,
        "stream": True,
        "temperature": 0.6,
        "top_p": 0.95,
        "top_k": 20,
    }).encode()

    req = urllib.request.Request(
        LLAMA_URL,
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )

    try:
        with urllib.request.urlopen(req) as resp:
            for line in resp:
                line = line.decode().strip()
                if not line.startswith("data:"):
                    continue
                line = line[5:].strip()
                if line == "[DONE]":
                    send({"type": "done"})
                    return
                try:
                    chunk = json.loads(line)
                    token = chunk["choices"][0]["delta"].get("content", "")
                    if token:
                        send({"type": "token", "content": token})
                except:
                    continue
    except Exception as e:
        send({"type": "error", "message": str(e)})

def main():
    if not check_server():
        send({"type": "error", "message": "llama-server not running on port 8081. Start it first with llama-server.exe"})
        sys.exit(1)

    log("Connected to llama-server on port 8081")
    send({"type": "ready"})

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            send({"type": "error", "message": f"invalid JSON: {line}"})
            continue

        if msg.get("type") == "generate":
            generate(msg.get("messages", []))
        elif msg.get("type") == "ping":
            send({"type": "pong"})
        elif msg.get("type") == "quit":
            break

if __name__ == "__main__":
    main()