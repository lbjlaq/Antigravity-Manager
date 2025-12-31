# Benchmarks

This folder contains a small, dependency-free benchmark runner to compare Anthropic-compatible proxy performance between:
- `Antigravity-Manager` (Tauri/Rust proxy, default port `8045`)
- `antigravity-claude-proxy-main` (Node/Express proxy, default port `8080`)

It measures (per request): HTTP status, TTFT (time-to-first token/event), total latency, SSE event counts, and usage tokens (including cache tokens when present).

## Requirements
- Node.js 18+ (uses built-in `fetch`)
- A running proxy exposing `POST /v1/messages` (Anthropic Messages API compatible)

## Quick start

Run against Antigravity-Manager (example) from `Antigravity-Manager/`:
```bash
API_KEY="sk-antigravity" node benchmarks/anthropic-proxy-bench.mjs \
  --profile text
```

Run against antigravity-claude-proxy-main (example):
```bash
node benchmarks/anthropic-proxy-bench.mjs \
  --base-url http://127.0.0.1:8080 \
  --profile caching \
  --runs 50 \
  --warmup 5
```

Write JSON output:
```bash
node benchmarks/anthropic-proxy-bench.mjs \
  --base-url http://127.0.0.1:8045 \
  --profile tool \
  --runs 50 \
  --warmup 5 \
  --out results.json
```

## Profiles
- `text`: single-turn streaming text.
- `caching`: two-turn style request with a large system prompt (useful to observe cache token fields).
- `tool`: tool-use request (forces a `tool_use` block in many setups).

## Standard defaults (Proxy A)
- `--base-url http://127.0.0.1:8045`
- `--model claude-3-5-sonnet-20241022`
- `--temperature 0`
- `--max-tokens 1024`
- `--runs 50 --warmup 5`
- Sends auth as `Authorization: Bearer $API_KEY` (single token).
