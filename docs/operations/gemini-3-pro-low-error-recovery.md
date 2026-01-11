# Gemini 3 Pro Low - Error Recovery Guide

Quick reference guide for troubleshooting `gemini-3-pro-low` tier-specific errors, quota management, and fallback strategies.

## Overview

Gemini 3 Pro Low tier provides cost-optimized reasoning (40-60% savings vs High tier) with **identical 32000 token thinking budget**. This guide covers Low tier specific error scenarios and recovery procedures.

## Error Categories

### 1. Quota Exhaustion (Low Tier Specific)

**Symptoms**:
- 429 errors with `quota_exhausted` message
- Low tier quota depleted before High tier

**Causes**:
- Separate quota pools per tier
- Low tier usage exceeds account limit
- Quota reset timing differences

**Recovery Steps**:
1. **Automatic Fallback**: System routes to High tier (if configured)
2. **Account Rotation**: Automatically tries next available account
3. **Wait for Reset**: Low tier quota resets independently (typically 24h)
4. **Manual Intervention**: Add more accounts or throttle request rate

**Log Query**:
```bash
# Find Low tier quota errors
grep "gemini-3-pro-low.*quota" logs/antigravity.log | tail -50

# Check quota by account
grep "gemini-3-pro-low" logs/antigravity.log | grep "account_email"
```

**Configuration**:
- Low tier fallback enabled by default
- Account rotation automatic
- Monitor dashboard for quota status

---

### 2. Routing Errors (Before Story-009-01)

**Symptoms**:
- Unknown model error for `gemini-low` or `gemini-3-low` aliases
- Unexpected routing to High tier

**Causes**:
- Convenience aliases not yet implemented (Story-009-01 pending)
- Only full model name `gemini-3-pro-low` supported

**Recovery Steps**:
1. **Use Full Name**: Replace aliases with `gemini-3-pro-low`
2. **Update Client Code**: Change model ID in application
3. **Wait for Story-009-01**: Aliases will be available after implementation

**Example Fix**:
```json
// Before (fails without Story-009-01)
{
  "model": "gemini-low"
}

// After (always works)
{
  "model": "gemini-3-pro-low"
}
```

**Note**: After Story-009-01 deployment, aliases `gemini-low` and `gemini-3-low` will route correctly.

---

### 3. Thinking Mode Confusion

**Symptoms**:
- Users expect `-thinking` suffix (like Claude)
- Confusion about thinking activation

**Causes**:
- Gemini uses **parameter-based** thinking (not model name suffix)
- Architectural difference from Claude models

**Recovery Steps**:
1. **Use Parameter**: Enable via `thinkingConfig` parameter
2. **Remove Suffix**: Don't append `-thinking` to model name
3. **Configure Budget**: Set `budget_tokens` in thinking config

**Correct Usage**:
```json
{
  "model": "gemini-3-pro-low",
  "thinking": {
    "type": "enabled",
    "budget_tokens": 16000
  }
}
```

**Wrong Usage**:
```json
{
  "model": "gemini-3-pro-low-thinking"  // ❌ NOT SUPPORTED
}
```

**Why Parameter-Based?**:
- More flexible (enable/disable per request)
- Fine-grained budget control
- Cleaner architecture
- Matches Gemini API native structure

---

### 4. Cost Budget Limits

**Symptoms**:
- Unexpected cost alerts for Low tier
- Higher costs than anticipated

**Causes**:
- Thinking mode increases costs (thinking tokens counted)
- `thinkingLevel` set too high for task requirements
- Not accounting for thinking overhead

**Recovery Steps**:
1. **Review Usage**: Check `thinkingLevel` in recent requests
2. **Optimize Level**: Use `low` or `medium` for simple tasks, `deep` for complex
3. **Switch Tiers**: Consider High tier for critical tasks requiring maximum quality
4. **Set Alerts**: Configure budget alerts per tier

**Cost Optimization**:
```yaml
Simple Tasks:
  thinkingLevel: "low"      # Minimal thinking overhead

Moderate Tasks:
  thinkingLevel: "medium"   # Balanced thinking

Complex Tasks:
  thinkingLevel: "deep"     # Maximum thinking
  OR
  tier: "gemini-3-pro-high" # Switch to High tier
```

**Budget Monitoring**:
```bash
# Track thinking usage
grep "thinkingLevel" logs/antigravity.log | grep "gemini-3-pro-low"

# Monitor cost patterns
grep "generation_time_ms" logs/antigravity.log | grep "gemini-3-pro-low"
```

---

### 5. Authentication Errors (401)

**Symptoms**:
- 401 Unauthorized errors
- Token refresh failures

**Recovery Steps**:
1. **Automatic Refresh**: System refreshes OAuth tokens automatically
2. **Account Rotation**: Tries next account if refresh fails
3. **Manual Reauth**: Re-authenticate account in dashboard if persistent

**Log Query**:
```bash
grep "401" logs/antigravity.log | grep "gemini-3-pro-low"
```

---

### 6. Rate Limiting (429)

**Symptoms**:
- 429 Too Many Requests errors
- Temporary rate limit blocks

**Recovery Steps**:
1. **Automatic Retry**: Exponential backoff (1s, 2s, 4s)
2. **Account Rotation**: Switch to different account
3. **Fallback to High**: Route to High tier if Low tier rate-limited

**Retry Configuration**:
- Max retries: 3
- Backoff: Exponential
- Total wait: ~7 seconds max

---

## Fallback Strategies

### Low → High Tier Fallback

**Trigger Conditions**:
- Low tier quota exhausted
- Low tier error rate >5% in last minute
- Critical task requiring maximum quality

**Automatic Behavior**:
```rust
if low_tier_quota_exhausted || low_tier_error_rate_high {
    route_to("gemini-3-pro-high")
}
```

**Manual Control**:
- Configure fallback in proxy settings
- Set error rate threshold
- Enable/disable per account

### Account Rotation Strategy

**Benefits**:
- Extended quota availability
- Improved reliability
- Automatic load distribution

**Configuration**:
- Account rotation enabled by default
- Round-robin selection
- Health-based prioritization

**Monitoring**:
```bash
# Check account rotation
grep "account_email" logs/antigravity.log | grep "gemini-3-pro-low" | tail -20

# Verify rotation working
grep "rotating.*account" logs/antigravity.log
```

---

## Monitoring & Diagnostics

### Essential Log Queries

**Find Low Tier Errors**:
```bash
# All Low tier errors
grep "gemini-3-pro-low" logs/antigravity.log | grep "ERROR"

# Error breakdown by type
grep "gemini-3-pro-low.*error_type" logs/antigravity.log | \
  awk -F'error_type=' '{print $2}' | awk '{print $1}' | sort | uniq -c
```

**Check Quota Status**:
```bash
# Quota messages
grep "quota.*low" logs/antigravity.log | tail -20

# Per-account quota
grep "account_email=user@example.com" logs/antigravity.log | grep "quota"
```

**Monitor Error Rate**:
```bash
# Errors in last hour
grep "$(date -u -v-1H '+%Y-%m-%d %H')" logs/antigravity.log | \
  grep "gemini-3-pro-low" | grep "ERROR" | wc -l

# Error rate trend
grep "gemini-3-pro-low" logs/antigravity.log | \
  grep "error_type" | tail -100 | cut -c1-13 | uniq -c
```

### Health Check Commands

**Verify Low Tier Availability**:
```bash
# Check model endpoint
curl http://localhost:8045/v1/models | jq '.data[] | select(.id | contains("low"))'

# Test Low tier request
curl -X POST http://localhost:8045/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini-3-pro-low",
    "messages": [{"role": "user", "content": "test"}]
  }'
```

**Monitor System Health**:
```bash
# Check proxy status
curl http://localhost:8045/health

# Verify account status
grep "account_status" logs/antigravity.log | tail -10
```

---

## Best Practices

### When to Use Low Tier

**✅ RECOMMENDED FOR**:
- Development and testing
- Code analysis and review
- Data extraction and parsing
- Problem-solving tasks
- Non-customer-facing reasoning
- Cost-sensitive applications
- Internal tools and automation

**❌ AVOID FOR**:
- Customer-facing responses
- Mission-critical production systems
- High-stakes decision making
- Brand-critical content
- Real-time latency requirements (<2s)

### Cost Optimization Tips

1. **Default to Low**: Use Low tier for 80% of requests
2. **Reserve High for Critical**: Save High tier for important 20%
3. **Optimize Thinking Level**: Start with `low`, increase only if needed
4. **Monitor Daily Quota**: Check usage patterns in dashboard
5. **Set Budget Alerts**: Configure alerts at 50%, 75%, 90% quota usage

### Quota Management

**Best Practices**:
- Monitor quota usage daily
- Rotate accounts for extended availability
- Plan for quota reset timing (typically 24h)
- Add accounts before hitting limits
- Use High tier fallback for critical operations

**Quota Characteristics**:
- Low tier quota separate from High tier
- Independent reset schedules
- Per-account quota pools
- Automatic rotation extends total quota

---

## Performance Expectations

| Metric | Low Tier | High Tier | Notes |
|--------|----------|-----------|-------|
| Response Time | 3-5s | 2-4s | Average for thinking mode |
| Quality | High | Very High | Both suitable for most tasks |
| Cost | $$ | $$$$ | 40-60% savings |
| Thinking Budget | 32000 | 32000 | **IDENTICAL** |
| Quota Reset | 24h | 24h | Independent schedules |
| Best For | Reasoning | Premium | Cost vs Quality tradeoff |

**Key Insight**: Same thinking budget means equal reasoning depth. Cost difference comes from base model quality, not thinking capability.

---

## Troubleshooting Checklist

Before escalating issues, verify:

- [ ] Low tier quota not exhausted (check dashboard)
- [ ] Using full model name `gemini-3-pro-low` (not alias before Story-009-01)
- [ ] Thinking mode via `thinkingConfig` parameter (not `-thinking` suffix)
- [ ] `thinkingLevel` appropriate for task (not always `deep`)
- [ ] Error logs reviewed for specific error type
- [ ] Account rotation working (check logs)
- [ ] Fallback to High tier configured
- [ ] Network connectivity to Google APIs verified
- [ ] OAuth tokens valid (no 401 errors)
- [ ] Rate limit not exceeded (no 429 errors)

---

## Common Issues Reference

| Issue | Cause | Solution | Priority |
|-------|-------|----------|----------|
| "Unknown model: gemini-low" | Aliases not implemented yet | Use `gemini-3-pro-low` | Medium |
| Unexpected high costs | `thinkingLevel` too high | Optimize to `low`/`medium` | Medium |
| Quality lower than expected | Low tier selected | Use High tier for critical tasks | Low |
| 429 quota errors | Quota exhausted | Wait for reset or rotate accounts | High |
| "-thinking suffix not working" | Parameter-based activation | Use `thinkingConfig` parameter | Low |
| Routing to High tier | No Low tier fallback | Check routing configuration | Medium |

---

## Error Recovery Workflow

```
ERROR DETECTED
    ↓
┌───────────────────┐
│ 1. Identify Error │
│   (Check logs)    │
└────────┬──────────┘
         ↓
┌───────────────────┐
│ 2. Check Category │
│ (Quota/Auth/Rate) │
└────────┬──────────┘
         ↓
┌───────────────────┐
│ 3. Auto Recovery  │
│ (Retry/Fallback)  │
└────────┬──────────┘
         ↓
    SUCCESS? ───Yes──→ RESOLVED
         │
         No
         ↓
┌───────────────────┐
│ 4. Manual Action  │
│ (Add accounts/    │
│  Adjust config)   │
└────────┬──────────┘
         ↓
    ESCALATE if needed
```

---

## Support Escalation

If errors persist after following this guide:

1. **Gather Information**:
   - Error logs (last 100 lines)
   - Account quota status
   - Request configuration
   - Timeline of issue

2. **Check Status**:
   - [Google Cloud Status](https://status.cloud.google.com/)
   - Antigravity Tools version
   - Recent configuration changes

3. **Contact Support**:
   - Provide error logs with context
   - Include model name and tier
   - Describe recovery attempts
   - Business impact assessment

4. **Escalation Path**:
   - Tech Lead → Product Owner → Engineering Team

---

## Additional Resources

- [Epic-009 Documentation](../epics/Epic-009-Gemini-3-Pro-Low-Compliance.md)
- [Gemini 3 Pro High Troubleshooting](../troubleshooting/gemini-3-pro-high-errors.md)
- [API Proxy Configuration](../api-proxy-configuration.md)
- [Account Management Guide](../account-management.md)
- [Quota Monitoring Dashboard](../dashboard-guide.md)

---

**Document Version**: 1.0
**Last Updated**: 2026-01-11
**Epic**: Epic-009 Story-009-04
**Status**: ✅ Complete
