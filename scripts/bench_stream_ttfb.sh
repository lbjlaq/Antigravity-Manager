#!/usr/bin/env bash
set -euo pipefail

# Streaming TTFB benchmark for Antigravity-Manager and other compatible proxies.
#
# Measures:
# - client-side TTFB (curl time_starttransfer): includes connect + TLS + server + upstream-to-first-byte
# - total time (curl time_total): for SSE this ends when the stream completes
#
# Usage examples:
#   API_KEY="..." ./scripts/bench_stream_ttfb.sh --base-url http://127.0.0.1:8080 --n 20
#   ./scripts/bench_stream_ttfb.sh --base-url http://127.0.0.1:8080 --auth off
#   ./scripts/bench_stream_ttfb.sh --base-url http://127.0.0.1:8080 --model claude-3-5-sonnet-20241022 --prompt "Hello"

BASE_URL="http://127.0.0.1:8080"
ENDPOINT="/v1/messages"
MODEL="claude-3-5-sonnet-20241022"
PROMPT="Say 'pong' and nothing else."
N=20
WARMUP=1
MAX_TOKENS=64
AUTH_MODE="auto" # auto|off|bearer|x-api-key
TIMEOUT_S=120
OUT_DIR="./benchmarks"

usage() {
  cat <<'EOF'
bench_stream_ttfb.sh

Options:
  --base-url URL         Base URL for the proxy (default: http://127.0.0.1:8080)
  --endpoint PATH        Endpoint path (default: /v1/messages)
  --model MODEL          Model name (default: claude-3-5-sonnet-20241022)
  --prompt TEXT          Prompt text (default: "Say 'pong' and nothing else.")
  --n N                  Number of measured runs (default: 20)
  --warmup N             Warmup runs excluded from stats (default: 1)
  --max-tokens N         max_tokens (default: 64)
  --timeout-s N          curl --max-time (default: 120)
  --out-dir DIR          Output dir (default: ./benchmarks)
  --auth MODE            auto|off|bearer|x-api-key (default: auto)

Env:
  API_KEY                Proxy API key (Bearer or x-api-key depending on --auth)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --base-url) BASE_URL="$2"; shift 2;;
    --endpoint) ENDPOINT="$2"; shift 2;;
    --model) MODEL="$2"; shift 2;;
    --prompt) PROMPT="$2"; shift 2;;
    --n) N="$2"; shift 2;;
    --warmup) WARMUP="$2"; shift 2;;
    --max-tokens) MAX_TOKENS="$2"; shift 2;;
    --timeout-s) TIMEOUT_S="$2"; shift 2;;
    --out-dir) OUT_DIR="$2"; shift 2;;
    --auth) AUTH_MODE="$2"; shift 2;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2;;
  esac
done

mkdir -p "$OUT_DIR"

ts="$(date +%Y%m%d_%H%M%S)"
safe_base="$(echo "$BASE_URL" | sed -e 's#https\?://##' -e 's#[/:]#_#g')"
out_file="${OUT_DIR}/stream_ttfb_${safe_base}_${ts}.csv"

url="${BASE_URL%/}${ENDPOINT}"

auth_header=()
case "$AUTH_MODE" in
  off)
    auth_header=()
    ;;
  bearer)
    if [[ -n "${API_KEY:-}" ]]; then auth_header=(-H "Authorization: Bearer ${API_KEY}"); fi
    ;;
  x-api-key)
    if [[ -n "${API_KEY:-}" ]]; then auth_header=(-H "x-api-key: ${API_KEY}"); fi
    ;;
  auto)
    if [[ -n "${API_KEY:-}" ]]; then
      # Antigravity-Manager accepts either Authorization or x-api-key.
      auth_header=(-H "Authorization: Bearer ${API_KEY}")
    fi
    ;;
  *)
    echo "Invalid --auth: ${AUTH_MODE}" >&2
    exit 2
    ;;
esac

body="$(jq -nc --arg model "$MODEL" --arg prompt "$PROMPT" --argjson max_tokens "$MAX_TOKENS" '{
  model: $model,
  max_tokens: $max_tokens,
  stream: true,
  messages: [{role:"user", content:$prompt}]
}')"

echo "url,run_kind,run_idx,http_code,ttfb_ms,total_ms" > "$out_file"

run_once() {
  local kind="$1"
  local idx="$2"

  # Use curl for client-side TTFB. Discard body, but keep streaming behavior with --no-buffer.
  local metrics
  metrics="$(
    curl -sS --no-buffer --max-time "$TIMEOUT_S" \
      -o /dev/null \
      -w "%{http_code} %{time_starttransfer} %{time_total}" \
      -H "Content-Type: application/json" \
      "${auth_header[@]}" \
      --data-binary "$body" \
      "$url" || true
  )"

  local http_code ttfb_s total_s
  http_code="$(echo "$metrics" | awk '{print $1}')"
  ttfb_s="$(echo "$metrics" | awk '{print $2}')"
  total_s="$(echo "$metrics" | awk '{print $3}')"

  # Convert seconds to ms (as float).
  local ttfb_ms total_ms
  ttfb_ms="$(python3 - <<PY
import sys
ttfb=float("${ttfb_s or 0}")
total=float("${total_s or 0}")
print(f"{ttfb*1000:.3f} {total*1000:.3f}")
PY
  )"
  printf "%s,%s,%s,%s,%s,%s\n" "$url" "$kind" "$idx" "$http_code" "$(echo "$ttfb_ms" | awk '{print $1}')" "$(echo "$ttfb_ms" | awk '{print $2}')" >> "$out_file"
}

echo "Warming up: ${WARMUP} run(s)..." >&2
for i in $(seq 1 "$WARMUP"); do
  run_once "warmup" "$i"
done

echo "Measuring: ${N} run(s)..." >&2
for i in $(seq 1 "$N"); do
  run_once "measure" "$i"
done

python3 - <<'PY' "$out_file"
import csv, statistics, sys
from math import ceil

path = sys.argv[1]
ttfb = []
total = []
codes = {}
with open(path, newline="") as f:
  for row in csv.DictReader(f):
    if row["run_kind"] != "measure":
      continue
    codes[row["http_code"]] = codes.get(row["http_code"], 0) + 1
    try:
      ttfb.append(float(row["ttfb_ms"]))
      total.append(float(row["total_ms"]))
    except ValueError:
      pass

def pct(values, p):
  if not values:
    return None
  s = sorted(values)
  k = max(0, min(len(s)-1, ceil(p/100*len(s)) - 1))
  return s[k]

def fmt(x):
  return "n/a" if x is None else f"{x:.3f}ms"

print("\n=== stream=true summary ===")
print(f"csv: {path}")
print(f"http_codes: {codes}")
print(f"ttfb: n={len(ttfb)} median={fmt(statistics.median(ttfb) if ttfb else None)} p95={fmt(pct(ttfb, 95))} min={fmt(min(ttfb) if ttfb else None)} max={fmt(max(ttfb) if ttfb else None)}")
print(f"total: n={len(total)} median={fmt(statistics.median(total) if total else None)} p95={fmt(pct(total, 95))} min={fmt(min(total) if total else None)} max={fmt(max(total) if total else None)}")
PY

echo "" >&2
echo "Saved: ${out_file}" >&2
