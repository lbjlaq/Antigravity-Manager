#!/bin/bash
# Test different request variants to find which one Google accepts for Claude models

set -e

echo "🔍 Testing Claude Model Request Variants"
echo "========================================"
echo ""

# Configuration
PROXY_URL="http://127.0.0.1:8045/v1/messages"
API_KEY="sk-antigravity"
TEST_MODEL="claude-sonnet-4-5"
LOG_DIR=".debug/claude_tests"

# Create log directory
mkdir -p "$LOG_DIR"

# Test variants
declare -a VARIANTS=(
    "baseline:agent:antigravity/1.11.9 windows/amd64"
    "partner_type:partner_model:antigravity/1.11.9 windows/amd64"
    "anthropic_type:anthropic:antigravity/1.11.9 windows/amd64"
    "claude_type:claude:antigravity/1.11.9 windows/amd64"
    "newer_version:agent:antigravity/1.13.0 windows/amd64"
    "darwin_os:agent:antigravity/1.11.9 darwin/arm64"
)

# Test payload
TEST_PAYLOAD='{
  "model": "'$TEST_MODEL'",
  "messages": [
    {
      "role": "user",
      "content": "Say hello in one word"
    }
  ],
  "max_tokens": 10,
  "stream": false
}'

echo "📝 Test Configuration:"
echo "  - Model: $TEST_MODEL"
echo "  - Proxy: $PROXY_URL"
echo "  - Variants: ${#VARIANTS[@]}"
echo ""

# Function to test a variant
test_variant() {
    local name=$1
    local request_type=$2
    local user_agent=$3
    local log_file="$LOG_DIR/${name}_$(date +%s).log"

    echo "Testing: $name"
    echo "  Request Type: $request_type"
    echo "  User Agent: $user_agent"

    # Set environment variables for the test
    export CLAUDE_REQUEST_TYPE="$request_type"
    export CLAUDE_USER_AGENT="$user_agent"

    # Make request and capture response
    local start_time=$(date +%s)
    local response=$(curl -s -w "\n\nHTTP_CODE:%{http_code}\nTIME:%{time_total}" \
        -X POST "$PROXY_URL" \
        -H "Content-Type: application/json" \
        -H "Authorization: Bearer $API_KEY" \
        -H "X-Test-Variant: $name" \
        -d "$TEST_PAYLOAD" 2>&1)
    local end_time=$(date +%s)

    # Extract HTTP code
    local http_code=$(echo "$response" | grep "HTTP_CODE:" | cut -d: -f2)
    local time_total=$(echo "$response" | grep "TIME:" | cut -d: -f2)

    # Save full response to log
    echo "=== Test Variant: $name ===" > "$log_file"
    echo "Date: $(date)" >> "$log_file"
    echo "Request Type: $request_type" >> "$log_file"
    echo "User Agent: $user_agent" >> "$log_file"
    echo "HTTP Code: $http_code" >> "$log_file"
    echo "Time: ${time_total}s" >> "$log_file"
    echo "" >> "$log_file"
    echo "=== Full Response ===" >> "$log_file"
    echo "$response" >> "$log_file"

    # Display result
    if [ "$http_code" == "200" ]; then
        echo "  ✅ SUCCESS (200 OK) in ${time_total}s"
        echo "  📄 Log: $log_file"
        echo ""
        return 0
    elif [ "$http_code" == "429" ]; then
        echo "  ❌ FAILED (429 Too Many Requests)"
        # Extract error message if present
        local error_msg=$(echo "$response" | grep -o '"message":"[^"]*"' | head -1)
        if [ ! -z "$error_msg" ]; then
            echo "     Error: $error_msg"
        fi
        echo "  📄 Log: $log_file"
        echo ""
        return 1
    else
        echo "  ⚠️  UNKNOWN ($http_code)"
        echo "  📄 Log: $log_file"
        echo ""
        return 2
    fi
}

# Run tests
success_count=0
failed_count=0
unknown_count=0

for variant in "${VARIANTS[@]}"; do
    IFS=':' read -r name request_type user_agent <<< "$variant"

    if test_variant "$name" "$request_type" "$user_agent"; then
        ((success_count++))
    else
        exit_code=$?
        if [ $exit_code -eq 1 ]; then
            ((failed_count++))
        else
            ((unknown_count++))
        fi
    fi

    # Small delay between requests
    sleep 2
done

# Summary
echo "========================================"
echo "📊 Test Summary"
echo "========================================"
echo "  ✅ Successful: $success_count"
echo "  ❌ Failed (429): $failed_count"
echo "  ⚠️  Unknown: $unknown_count"
echo ""
echo "📂 All logs saved to: $LOG_DIR"
echo ""

if [ $success_count -gt 0 ]; then
    echo "🎉 Found working variant(s)! Check logs for details."
    exit 0
else
    echo "❌ No working variants found. Need to investigate further."
    exit 1
fi
