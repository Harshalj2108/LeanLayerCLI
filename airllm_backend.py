#!/usr/bin/env python3
"""
AirLLM backend — reads JSON-lines from stdin, streams tokens to stdout.
"""

import sys
import json
import os
from airllm import AutoModel

model = None

def load_model(model_path: str):
    global model
    sys.stderr.write(f"[backend] Loading model from {model_path}\n")
    sys.stderr.flush()
    model = AutoModel.from_pretrained(model_path)
    sys.stderr.write("[backend] Model loaded\n")
    sys.stderr.flush()

def send(obj: dict):
    print(json.dumps(obj), flush=True)

def generate(messages: list):
    # Build a simple prompt from message history
    prompt = ""
    for msg in messages:
        role = msg["role"]
        content = msg["content"]
        if role == "user":
            prompt += f"<start_of_turn>user\n{content}<end_of_turn>\n"
        elif role == "assistant":
            prompt += f"<start_of_turn>model\n{content}<end_of_turn>\n"
        elif role == "system":
            prompt += f"{content}\n"
    prompt += "<start_of_turn>model\n"

    try:
        # AirLLM generates token by token
        for token in model.generate(prompt, max_new_tokens=1024):
            send({"type": "token", "content": token})
        send({"type": "done"})
    except Exception as e:
        send({"type": "error", "message": str(e)})

def main():
    if len(sys.argv) < 2:
        sys.stderr.write("Usage: airllm_backend.py <model_path>\n")
        sys.exit(1)

    load_model(sys.argv[1])
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