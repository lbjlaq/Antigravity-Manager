#!/usr/bin/env bash
set -euo pipefail

# Synthetic Claude Code-like streaming benchmark for /v1/messages.
#
# Focus:
# - stream=true TTFB + total time
# - multi-turn conversation within a stable metadata.user_id session
# - includes (1) client tool: shell (tool_use + tool_result replay)
# - includes (2) server web_search tool (googleSearch injection via web_search_20250305)
#
# Output:
# - single CSV with per-turn timings per proxy
# - summary (median/p95) printed to stdout
#
# Requires: curl, jq, python3

BASE_URL_A="http://127.0.0.1:8080"
BASE_URL_B=""
MODEL="claude-3-5-sonnet-20241022"
N=20
WARMUP=1
TIMEOUT_S=120
OUT_DIR="./benchmarks"
AUTH_MODE="auto" # auto|off|bearer|x-api-key (applies to both; can override via AUTH_MODE_A/B env)

usage() {
  cat <<'EOF'
bench_claude_stream_synth.sh

Options:
  --base-url-a URL         Proxy A base URL (default: http://127.0.0.1:8080)
  --base-url-b URL         Proxy B base URL (optional; if set, runs comparison)
  --model MODEL            Claude model string (default: claude-3-5-sonnet-20241022)
  --n N                    Number of measured runs (default: 20)
  --warmup N               Warmup runs excluded from stats (default: 1)
  --timeout-s N            curl --max-time (default: 120)
  --out-dir DIR            Output dir (default: ./benchmarks)
  --auth MODE              auto|off|bearer|x-api-key (default: auto)

Env (shared or per-proxy):
  API_KEY                  Default API key (used for both A and B if per-proxy not set)
  API_KEY_A, API_KEY_B     API keys per proxy
  AUTH_MODE_A, AUTH_MODE_B Override auth mode per proxy
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --base-url-a) BASE_URL_A="$2"; shift 2;;
    --base-url-b) BASE_URL_B="$2"; shift 2;;
    --model) MODEL="$2"; shift 2;;
    --n) N="$2"; shift 2;;
    --warmup) WARMUP="$2"; shift 2;;
    --timeout-s) TIMEOUT_S="$2"; shift 2;;
    --out-dir) OUT_DIR="$2"; shift 2;;
    --auth) AUTH_MODE="$2"; shift 2;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2;;
  esac
done

mkdir -p "$OUT_DIR"
RAW_DIR="${OUT_DIR}/raw"
mkdir -p "$RAW_DIR"

ts="$(date +%Y%m%d_%H%M%S)"
csv="${OUT_DIR}/claude_stream_synth_${ts}.csv"

echo "proxy,run_kind,run_idx,turn,http_code,ttfb_ms,total_ms,shell_tool_uses,server_web_search_blocks" > "$csv"

build_auth_header() {
  local mode="$1"
  local key="$2"
  local -a hdr=()
  case "$mode" in
    off) hdr=();;
    bearer) [[ -n "$key" ]] && hdr=(-H "Authorization: Bearer ${key}");;
    x-api-key) [[ -n "$key" ]] && hdr=(-H "x-api-key: ${key}");;
    auto)
      [[ -n "$key" ]] && hdr=(-H "Authorization: Bearer ${key}")
      ;;
    *)
      echo "Invalid auth mode: $mode" >&2
      exit 2
      ;;
  esac
  printf "%s\0" "${hdr[@]}"
}

sec_to_ms_pair() {
  python3 - <<PY
ttfb=float("${1:-0}")
total=float("${2:-0}")
print(f"{ttfb*1000:.3f} {total*1000:.3f}")
PY
}

run_stream_request() {
  local proxy_label="$1"
  local base_url="$2"
  local auth_mode="$3"
  local api_key="$4"
  local run_kind="$5"
  local run_idx="$6"
  local turn="$7"
  local json_body="$8"

  local url="${base_url%/}/v1/messages"
  local safe_base
  safe_base="$(echo "$base_url" | sed -e 's#https\?://##' -e 's#[/:]#_#g')"
  local body_file="${RAW_DIR}/sse_${ts}_${proxy_label}_${safe_base}_${run_kind}_${run_idx}_t${turn}.log"

  # Rehydrate auth header array from nul-separated output.
  local -a auth_header=()
  while IFS= read -r -d '' item; do auth_header+=("$item"); done < <(build_auth_header "$auth_mode" "$api_key")

  local metrics
  metrics="$(
    curl -sS --no-buffer --max-time "$TIMEOUT_S" \
      -o "$body_file" \
      -w "%{http_code} %{time_starttransfer} %{time_total}" \
      -H "Content-Type: application/json" \
      "${auth_header[@]}" \
      --data-binary "$json_body" \
      "$url" || true
  )"

  local http_code ttfb_s total_s
  http_code="$(echo "$metrics" | awk '{print $1}')"
  ttfb_s="$(echo "$metrics" | awk '{print $2}')"
  total_s="$(echo "$metrics" | awk '{print $3}')"

  local ms_pair ttfb_ms total_ms
  ms_pair="$(sec_to_ms_pair "$ttfb_s" "$total_s")"
  ttfb_ms="$(echo "$ms_pair" | awk '{print $1}')"
  total_ms="$(echo "$ms_pair" | awk '{print $2}')"

  printf "%s\n" "$body_file"
  printf "%s\n" "$http_code"
  printf "%s\n" "$ttfb_ms"
  printf "%s\n" "$total_ms"
}

count_sse_blocks() {
  local sse_file="$1"
  python3 - <<'PY' "$sse_file"
import json, sys

path = sys.argv[1]
shell_tool_uses = 0
server_web_search_blocks = 0

with open(path, "r", encoding="utf-8", errors="ignore") as f:
  for line in f:
    line = line.strip()
    if not line.startswith("data: "):
      continue
    data = line[6:].strip()
    if not data or data == "[DONE]":
      continue
    try:
      obj = json.loads(data)
    except Exception:
      continue

    if obj.get("type") == "content_block_start":
      cb = obj.get("content_block") or {}
      if cb.get("type") == "tool_use" and cb.get("name") == "shell":
        shell_tool_uses += 1
      if cb.get("type") == "server_tool_use" and cb.get("name") == "web_search":
        server_web_search_blocks += 1

print(shell_tool_uses, server_web_search_blocks)
PY
}

extract_first_shell_tool_use_id() {
  local sse_file="$1"
  python3 - <<'PY' "$sse_file"
import json, sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8", errors="ignore") as f:
  for line in f:
    line = line.strip()
    if not line.startswith("data: "):
      continue
    data = line[6:].strip()
    if not data or data == "[DONE]":
      continue
    try:
      obj = json.loads(data)
    except Exception:
      continue
    if obj.get("type") == "content_block_start":
      cb = obj.get("content_block") or {}
      if cb.get("type") == "tool_use" and cb.get("name") == "shell":
        tool_id = cb.get("id") or ""
        print(tool_id)
        sys.exit(0)
print("")
PY
}

run_scenario_for_proxy() {
  local proxy_label="$1"
  local base_url="$2"
  local run_kind="$3"
  local run_idx="$4"

  local api_key="${API_KEY:-}"
  if [[ "$proxy_label" == "A" ]]; then
    api_key="${API_KEY_A:-$api_key}"
  else
    api_key="${API_KEY_B:-$api_key}"
  fi

  local auth_mode="${AUTH_MODE}"
  if [[ "$proxy_label" == "A" && -n "${AUTH_MODE_A:-}" ]]; then auth_mode="${AUTH_MODE_A}"; fi
  if [[ "$proxy_label" == "B" && -n "${AUTH_MODE_B:-}" ]]; then auth_mode="${AUTH_MODE_B}"; fi

  local session_id="bench_synth_${ts}_${proxy_label}"

  # Turn 1: baseline response (no tools)
  local body1
  body1="$(jq -nc --arg model "$MODEL" --arg sid "$session_id" '{
    model: $model,
    stream: true,
    max_tokens: 128,
    metadata: { user_id: $sid },
    messages: [{ role:"user", content:"Reply with exactly: OK" }]
  }')"

  local out1
  out1="$(run_stream_request "$proxy_label" "$base_url" "$auth_mode" "$api_key" "$run_kind" "$run_idx" 1 "$body1")"
  local sse1 http1 ttfb1 total1
  sse1="$(echo "$out1" | sed -n '1p')"
  http1="$(echo "$out1" | sed -n '2p')"
  ttfb1="$(echo "$out1" | sed -n '3p')"
  total1="$(echo "$out1" | sed -n '4p')"
  read -r shell1 web1 <<<"$(count_sse_blocks "$sse1")"
  echo "${proxy_label},${run_kind},${run_idx},1,${http1},${ttfb1},${total1},${shell1},${web1}" >> "$csv"

  # Turn 2: shell tool_use
  local shell_tool
  shell_tool="$(jq -nc '{
    name: "shell",
    description: "Run a shell command. Input: {command: [string]}",
    input_schema: {
      type: "object",
      properties: { command: { type: "array", items: { type: "string" } } },
      required: ["command"]
    }
  }')"
  local body2
  body2="$(jq -nc --arg model "$MODEL" --arg sid "$session_id" --argjson tool "$shell_tool" '{
    model: $model,
    stream: true,
    max_tokens: 256,
    metadata: { user_id: $sid },
    tools: [$tool],
    messages: [{
      role:"user",
      content:"You MUST call the shell tool with command [\"echo\",\"hello_synth\"]. After calling the tool, wait for tool results."
    }]
  }')"

  local out2
  out2="$(run_stream_request "$proxy_label" "$base_url" "$auth_mode" "$api_key" "$run_kind" "$run_idx" 2 "$body2")"
  local sse2 http2 ttfb2 total2
  sse2="$(echo "$out2" | sed -n '1p')"
  http2="$(echo "$out2" | sed -n '2p')"
  ttfb2="$(echo "$out2" | sed -n '3p')"
  total2="$(echo "$out2" | sed -n '4p')"
  read -r shell2 web2 <<<"$(count_sse_blocks "$sse2")"
  echo "${proxy_label},${run_kind},${run_idx},2,${http2},${ttfb2},${total2},${shell2},${web2}" >> "$csv"

  local tool_use_id
  tool_use_id="$(extract_first_shell_tool_use_id "$sse2")"

  # Turn 3: replay tool_result for shell + request web_search
  local tool_result_block="null"
  if [[ -n "$tool_use_id" ]]; then
    tool_result_block="$(jq -nc --arg tid "$tool_use_id" '{
      type: "tool_result",
      tool_use_id: $tid,
      content: [{type:"text", text:"hello_synth\n"}]
    }')"
  fi
  local web_search_tool
  web_search_tool="$(jq -nc '{ type: "web_search_20250305" }')"

  local body3
  body3="$(jq -nc --arg model "$MODEL" --arg sid "$session_id" --argjson tr "$tool_result_block" --argjson ws "$web_search_tool" '{
    model: $model,
    stream: true,
    max_tokens: 512,
    metadata: { user_id: $sid },
    tools: [$ws],
    messages: [{
      role:"user",
      content: ( ($tr|type) == "object"
        ? [$tr, {type:"text", text:"Now perform a web_search for \"OpenAI Codex CLI\" and return a 1-sentence summary."}]
        : [{type:"text", text:"Now perform a web_search for \"OpenAI Codex CLI\" and return a 1-sentence summary."}]
      )
    }]
  }')"

  local out3
  out3="$(run_stream_request "$proxy_label" "$base_url" "$auth_mode" "$api_key" "$run_kind" "$run_idx" 3 "$body3")"
  local sse3 http3 ttfb3 total3
  sse3="$(echo "$out3" | sed -n '1p')"
  http3="$(echo "$out3" | sed -n '2p')"
  ttfb3="$(echo "$out3" | sed -n '3p')"
  total3="$(echo "$out3" | sed -n '4p')"
  read -r shell3 web3 <<<"$(count_sse_blocks "$sse3")"
  echo "${proxy_label},${run_kind},${run_idx},3,${http3},${ttfb3},${total3},${shell3},${web3}" >> "$csv"
}

echo "Warming up: ${WARMUP} run(s)..." >&2
for i in $(seq 1 "$WARMUP"); do
  run_scenario_for_proxy "A" "$BASE_URL_A" "warmup" "$i"
  if [[ -n "$BASE_URL_B" ]]; then
    run_scenario_for_proxy "B" "$BASE_URL_B" "warmup" "$i"
  fi
done

echo "Measuring: ${N} run(s)..." >&2
for i in $(seq 1 "$N"); do
  run_scenario_for_proxy "A" "$BASE_URL_A" "measure" "$i"
  if [[ -n "$BASE_URL_B" ]]; then
    run_scenario_for_proxy "B" "$BASE_URL_B" "measure" "$i"
  fi
done

python3 - <<'PY' "$csv"
import csv, statistics, sys
from math import ceil

path = sys.argv[1]
rows = []
with open(path, newline="") as f:
  for r in csv.DictReader(f):
    if r["run_kind"] != "measure":
      continue
    rows.append(r)

def pct(values, p):
  if not values:
    return None
  s = sorted(values)
  k = max(0, min(len(s)-1, ceil(p/100*len(s)) - 1))
  return s[k]

def fmt(x):
  return "n/a" if x is None else f"{x:.3f}ms"

def summarize(proxy, turn):
  ttfb = []
  total = []
  codes = {}
  for r in rows:
    if r["proxy"] != proxy or r["turn"] != str(turn):
      continue
    codes[r["http_code"]] = codes.get(r["http_code"], 0) + 1
    try:
      ttfb.append(float(r["ttfb_ms"]))
      total.append(float(r["total_ms"]))
    except ValueError:
      pass
  print(f"\n== proxy {proxy} turn {turn} ==")
  print(f"http_codes: {codes}")
  print(f"ttfb: n={len(ttfb)} median={fmt(statistics.median(ttfb) if ttfb else None)} p95={fmt(pct(ttfb,95))} min={fmt(min(ttfb) if ttfb else None)} max={fmt(max(ttfb) if ttfb else None)}")
  print(f"total: n={len(total)} median={fmt(statistics.median(total) if total else None)} p95={fmt(pct(total,95))} min={fmt(min(total) if total else None)} max={fmt(max(total) if total else None)}")

def tool_success(proxy):
  ok = 0
  total = 0
  for r in rows:
    if r["proxy"] != proxy or r["turn"] != "2":
      continue
    total += 1
    try:
      ok += 1 if int(r["shell_tool_uses"]) >= 1 else 0
    except Exception:
      pass
  return ok, total

print("\n=== claude stream synth summary ===")
print(f"csv: {path}")
proxies = sorted(set(r["proxy"] for r in rows))
for p in proxies:
  for t in (1,2,3):
    summarize(p, t)
  ok, tot = tool_success(p)
  print(f"\nproxy {p}: shell tool_use detected in turn2: {ok}/{tot}")
PY

echo "" >&2
echo "Saved: ${csv}" >&2
