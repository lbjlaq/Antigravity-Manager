# Setup Guide: Story-006-01 Live API Validation

Quick start guide for executing gemini-2.5-flash-lite-thinking validation tests.

---

## Prerequisites Checklist

Before running tests, ensure:

- [ ] **Antigravity Application Installed**
  - Application is running
  - Can access via GUI

- [ ] **Valid Google Account**
  - OAuth token is valid (not expired)
  - Vertex AI API enabled in GCP project
  - Account configured in Antigravity

- [ ] **Proxy Server Configuration**
  - Know API key (from Antigravity settings)
  - Proxy port is 8045 (default)

- [ ] **Sufficient Quota**
  - At least 10K tokens available
  - No rate limit restrictions

---

## Step-by-Step Setup

### Step 1: Start Antigravity Proxy Server

**Option A: Via GUI (Recommended)**
1. Open Antigravity application
2. Navigate to "API Proxy" tab
3. Click "Start Server" button
4. Verify status shows "Running" on port 8045

**Option B: Verify Server is Running**
```bash
# Check if port 8045 is in use
lsof -ti:8045

# Should return a process ID if server is running
# If nothing returned, server is NOT running
```

### Step 2: Get API Key

**Via GUI**:
1. Open Antigravity → Settings
2. Find "API Proxy" section
3. Copy "API Key" value

**Set Environment Variable**:
```bash
export ANTIGRAVITY_API_KEY="your-copied-api-key-here"
```

### Step 3: Verify Account Status

**Via GUI**:
1. Open Antigravity → Accounts tab
2. Verify at least one account shows:
   - ✅ Valid token (no "Expired" warning)
   - ✅ Quota remaining > 10K
   - ✅ Status: Active

**If Token Expired**:
1. Click "Refresh Token" button
2. Complete OAuth flow if prompted
3. Verify token is now valid

---

## Running the Tests

### Quick Validation (All Phases)

```bash
cd /Users/r2d2/Documents/Code_Projects/00_mcp/Antigravity-Manager/src-tauri

cargo test --test validate_flash_lite_thinking test_comprehensive_validation -- --ignored --nocapture
```

**Expected Output**:
```
╔════════════════════════════════════════════════════════════════╗
║   Live API Validation: gemini-2.5-flash-lite-thinking         ║
║   Story-006-01 - Epic-006 Foundation Validation               ║
╚════════════════════════════════════════════════════════════════╝

🔍 PHASE 1: Model Existence Test
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📤 Sending request to gemini-2.5-flash-lite-thinking...
✅ SUCCESS: Model accepted request
✅ AC-1 PASS: Model exists and accepts requests

🧠 PHASE 2: Thinking Capability Test
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📤 Sending reasoning request...
✅ Response contains thinking blocks
✅ AC-2 PASS: Thinking capability validated

📊 PHASE 3: Budget Limit Validation
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📝 Subtest 1: Budget below limit (2000)
   ✅ Budget respected
📝 Subtest 2: Budget at limit (24576)
   ✅ Budget respected
📝 Subtest 3: Budget above limit (32000 - CRITICAL TEST)
   ✅ PATTERN MATCHING CONFIRMED!
✅ AC-3 PASS: Budget limit confirmed as 24576

╔════════════════════════════════════════════════════════════════╗
║                    VALIDATION COMPLETE                         ║
╚════════════════════════════════════════════════════════════════╝
```

**Duration**: ~60 seconds
**Token Usage**: ~8500 tokens

---

## Troubleshooting

### Error: "Failed to connect to proxy"

**Cause**: Proxy server not running

**Solution**:
1. Start Antigravity application
2. Go to "API Proxy" tab
3. Click "Start Server"
4. Retry test

---

### Error: "401 Unauthorized"

**Cause**: Invalid or expired OAuth token

**Solution**:
1. Open Antigravity → Accounts
2. Click "Refresh Token" for your account
3. Complete OAuth flow
4. Retry test

---

### Error: "403 Forbidden - API not enabled"

**Cause**: Vertex AI API not enabled in GCP project

**Solution**:
1. Go to [Google Cloud Console](https://console.cloud.google.com/)
2. Select your project
3. Navigate to "APIs & Services" → "Library"
4. Search for "Vertex AI API"
5. Click "Enable"
6. Wait 1-2 minutes for propagation
7. Retry test

---

### Error: "429 Too Many Requests"

**Cause**: Quota exceeded or rate limit hit

**Solution**:
1. Wait 60 seconds for rate limit reset
2. Check account quota in Antigravity
3. Use different account if available
4. Retry test

---

### Error: "Model not found" or "Invalid model"

**Cause**: Model `gemini-2.5-flash-lite-thinking` not available

**Impact**: 🚨 **CRITICAL - BLOCKS Epic-006**

**Actions**:
1. ❌ DO NOT proceed with Epic-006
2. Document error in validation report
3. Escalate to product owner
4. Investigate model availability:
   - Check Gemini model catalog
   - Try variations: `gemini-2.5-flash-lite-exp-thinking`
   - Verify model is released (not preview)

---

## After Successful Execution

### 1. Capture Evidence

**Screenshots**:
```bash
# Take screenshot of terminal output
# Save as: evidence-terminal-output.png
```

**Save API Responses**:
- Test output is logged to console with `--nocapture`
- Copy relevant sections to validation report

### 2. Update Documentation

**Files to Update**:
1. `docs/validation/gemini-2.5-flash-lite-thinking-validation-report.md`
   - Fill in "Test Results" section
   - Add evidence screenshots
   - Update "Confidence Assessment" to 100%
   - Set "Decision" to GO

2. `docs/antigravity/workflows/models/gemini/gemini-2.5-flash-lite-thinking-COMPARISON.md`
   - Update overall confidence: 95% → 100%
   - Add validation date
   - Add reference to validation report

3. `docs/stories/Story-006-01-live-api-validation.md`
   - Update status to "✅ IMPLEMENTED"
   - Add actual results summary

4. `docs/epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md`
   - Update Story-006-01 status
   - Update progress: 0/6 → 1/6

### 3. Commit Changes

```bash
cd /Users/r2d2/Documents/Code_Projects/00_mcp/Antigravity-Manager

git add docs/validation/
git add docs/stories/Story-006-01-live-api-validation.md
git add docs/antigravity/workflows/models/gemini/gemini-2.5-flash-lite-thinking-COMPARISON.md
git add docs/epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md
git add src-tauri/tests/validate_flash_lite_thinking.rs

git commit -m "docs(story-006-01): validate gemini-2.5-flash-lite-thinking via live API

- ✅ AC-1: Model exists and accepts requests
- ✅ AC-2: Thinking capability confirmed
- ✅ AC-3: Budget limit verified as 24576
- 📊 Confidence boost: 95% → 100%
- 🚀 Decision: GO for Epic-006

Test Results:
- All 3 validation phases passed
- Pattern matching prediction validated
- Model performs as expected

Evidence:
- Test script: src-tauri/tests/validate_flash_lite_thinking.rs
- Validation report: docs/validation/gemini-2.5-flash-lite-thinking-validation-report.md

Ref: docs/stories/Story-006-01-live-api-validation.md
Unblocks: Story-006-02, Story-006-03, Story-006-04, Story-006-05, Story-006-06"
```

### 4. Notify Team

**Message Template**:
```
✅ Story-006-01 COMPLETE - GO for Epic-006

Model: gemini-2.5-flash-lite-thinking
Status: VALIDATED ✅

Results:
✅ AC-1: Model exists and accepts requests
✅ AC-2: Thinking capability confirmed
✅ AC-3: Budget limit verified as 24576

Confidence: 95% → 100%
Decision: GO for Epic-006

Next: Stories 002-006 are now UNBLOCKED

Documentation:
- Validation Report: docs/validation/gemini-2.5-flash-lite-thinking-validation-report.md
- Test Script: src-tauri/tests/validate_flash_lite_thinking.rs
```

---

## Quick Reference

### Test Commands

```bash
# Full validation (recommended)
cargo test --test validate_flash_lite_thinking test_comprehensive_validation -- --ignored --nocapture

# Individual phases
cargo test --test validate_flash_lite_thinking test_phase_1_model_exists -- --ignored --nocapture
cargo test --test validate_flash_lite_thinking test_phase_2_thinking_capability -- --ignored --nocapture
cargo test --test validate_flash_lite_thinking test_phase_3_budget_limits -- --ignored --nocapture
```

### Environment Variables

```bash
# Optional overrides
export ANTIGRAVITY_PROXY_HOST="127.0.0.1"
export ANTIGRAVITY_PROXY_PORT="8045"
export ANTIGRAVITY_API_KEY="your-api-key"
```

### Key Files

```
src-tauri/tests/validate_flash_lite_thinking.rs     # Test script
docs/validation/gemini-2.5-flash-lite-thinking-validation-report.md  # Results
docs/validation/SETUP-GUIDE.md                      # This file
docs/stories/Story-006-01-live-api-validation.md    # Story spec
```

---

**Created**: 2026-01-11
**For**: Story-006-01 - Live API Validation
**Epic**: Epic-006 - Gemini 2.5 Flash Lite Thinking Optimizations
