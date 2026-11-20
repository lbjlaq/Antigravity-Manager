#!/usr/bin/env python3
"""Check gateway authentication and real model generation without exposing keys."""

import argparse
import json
import pathlib
import sys
import time
import urllib.error
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--env-file", type=pathlib.Path, default=pathlib.Path(__file__).resolve().parents[1] / "docker/.env")
    parser.add_argument("--base-url", default="http://127.0.0.1:8045")
    parser.add_argument("--model", default="gemini-3.8-flash-low")
    parser.add_argument("--stream", action="store_true")
    args = parser.parse_args()
    values = {}
    for line in args.env_file.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            key, value = line.split("=", 1)
            values[key.strip()] = value.strip().strip("\"'")
    key = values.get("API_KEY")
    if not key:
        raise ValueError("Set API_KEY in the local env file first")

    base = args.base_url.rstrip("/")
    # The local gateway must be contacted directly, independent of shell proxies.
    client = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with client.open(base + "/health", timeout=10) as response:
        if json.load(response).get("status") != "ok":
            raise ValueError("Gateway health check failed")
    try:
        with client.open(base + "/v1/models", timeout=10):
            raise ValueError("Unauthenticated model requests must be rejected")
    except urllib.error.HTTPError as error:
        if error.code != 401:
            raise ValueError(f"Expected unauthenticated HTTP 401, got {error.code}") from None

    expected = "ANTIGRAVITY_PROXY_OK"
    payload = {
        "model": args.model,
        "messages": [{"role": "user", "content": f"Reply with exactly {expected} and nothing else."}],
        "max_tokens": 256,
        "stream": args.stream,
    }
    request = urllib.request.Request(
        base + "/v1/chat/completions",
        data=json.dumps(payload).encode(),
        headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"},
    )
    start = time.monotonic()
    try:
        with client.open(request, timeout=90) as response:
            if args.stream:
                parts, done = [], False
                for line in response:
                    line = line.decode().strip()
                    if not line.startswith("data:"):
                        continue
                    data = line[5:].strip()
                    if data == "[DONE]":
                        done = True
                        break
                    event = json.loads(data)
                    if "error" in event:
                        raise ValueError("Gateway returned an error inside the stream")
                    for choice in event.get("choices", []):
                        parts.append(choice.get("delta", {}).get("content") or "")
                if not done:
                    raise ValueError("Streaming response ended without [DONE]")
                content = "".join(parts)
            else:
                data = json.load(response)
                content = data.get("choices", [{}])[0].get("message", {}).get("content") or ""
    except urllib.error.HTTPError as error:
        try:
            message = json.loads(error.read()).get("error", {}).get("message", "")
        except (ValueError, AttributeError):
            message = "Non-JSON error response"
        for name in ("API_KEY", "WEB_PASSWORD"):
            if values.get(name):
                message = message.replace(values[name], "<REDACTED>")
        raise ValueError(f"Generation failed: HTTP {error.code}: {message[:600]}") from None
    if content.strip() != expected:
        raise ValueError("Model response did not contain the expected verification text")
    print(json.dumps({"result": "PASS", "model": args.model, "stream": args.stream, "content": expected, "seconds": round(time.monotonic() - start, 2)}))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, IndexError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
