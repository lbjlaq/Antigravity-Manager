# Image Generation Operations Runbook

**Epic**: Epic-007 - Gemini 3 Pro Image Compliance
**Version**: v3.3.20
**Last Updated**: 2026-01-11
**Audience**: Operations, DevOps, SRE

---

## Overview

Operational runbook for deploying, monitoring, and troubleshooting Gemini 3 Pro Image generation in Antigravity Tools. Covers deployment procedures, monitoring dashboards, common issues, and incident response.

---

## Table of Contents

1. [Deployment Procedures](#deployment-procedures)
2. [Monitoring & Alerting](#monitoring--alerting)
3. [Common Issues & Resolutions](#common-issues--resolutions)
4. [Cache Management](#cache-management)
5. [Performance Troubleshooting](#performance-troubleshooting)
6. [Security Incident Response](#security-incident-response)
7. [Rollback Procedures](#rollback-procedures)
8. [Maintenance Tasks](#maintenance-tasks)

---

## Deployment Procedures

### Pre-Deployment Checklist

**Code Quality** (verified in Story-007-05):
- [ ] All 217 unit tests passing (`cargo test --lib`)
- [ ] Compilation successful (zero errors)
- [ ] Code formatted (`cargo fmt`)
- [ ] Configuration validated

**Environment Preparation**:
- [ ] Target environment identified (dev/staging/prod)
- [ ] Configuration profile selected (see Configuration Guide)
- [ ] Environment variables prepared
- [ ] Cache directory created with proper permissions
- [ ] API keys secured in environment

**Integration Validation**:
- [ ] All 4 Epic-007 stories complete (007-01 through 007-04)
- [ ] Safety settings tested
- [ ] Caching validated
- [ ] Error logging verified
- [ ] E2E test infrastructure ready

### Deployment Steps

#### Development Environment

```bash
# 1. Set environment variables
export GEMINI_IMAGE_SAFETY_THRESHOLD=OFF
export CACHE_BACKEND=filesystem
export CACHE_MAX_SIZE_MB=50
export CACHE_TTL_SECONDS=1800

# 2. Create cache directory
mkdir -p ~/.antigravity_tools/image_cache
chmod 755 ~/.antigravity_tools/image_cache

# 3. Start application
cd /path/to/Antigravity-Manager
npm run tauri dev

# 4. Verify startup
# Check logs for "Proxy server started"
# Check cache initialization messages
```

#### Staging Environment

```bash
# 1. Backup current configuration
cp ~/.antigravity_tools/config.json ~/.antigravity_tools/config.json.backup

# 2. Set production-like configuration
export GEMINI_IMAGE_SAFETY_THRESHOLD=MEDIUM
export CACHE_BACKEND=filesystem
export CACHE_MAX_SIZE_MB=200
export CACHE_TTL_SECONDS=3600

# 3. Deploy application
npm run tauri build
# Install built application

# 4. Start proxy server
# (Auto-starts with application or manual start via UI)

# 5. Run smoke tests
curl http://localhost:8045/v1/images/generations \
  -H "Authorization: Bearer $ANTIGRAVITY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini-3-pro-image",
    "prompt": "Test image: simple geometric shapes",
    "n": 1
  }'

# 6. Monitor for 30 minutes
# Watch logs, check cache behavior, verify safety filtering
```

#### Production Environment

```bash
# 1. Create deployment plan
# Document: Current version, new version, rollback plan
# Notify: Team, stakeholders about deployment window

# 2. Backup production data
tar -czf antigravity-backup-$(date +%Y%m%d).tar.gz \
  ~/.antigravity_tools/config.json \
  ~/.antigravity_tools/accounts/*.json \
  ~/.antigravity_tools/antigravity.db

# 3. Set production configuration
export GEMINI_IMAGE_SAFETY_THRESHOLD=MEDIUM
export CACHE_BACKEND=filesystem
export CACHE_MAX_SIZE_MB=500
export CACHE_TTL_SECONDS=7200

# 4. Deploy application
npm run tauri build --target universal-apple-darwin
# Install .dmg or distribute to users

# 5. Verify critical paths
# Test basic generation
# Test cache hit/miss
# Test safety filtering
# Test error logging

# 6. Monitor deployment
# First 1 hour: Active monitoring (every 5 minutes)
# Next 6 hours: Periodic checks (every 30 minutes)
# Ongoing: Automated monitoring and alerts
```

### Post-Deployment Verification

**Functional Checks** (5-10 minutes):
```bash
# 1. Basic image generation
curl -X POST http://localhost:8045/v1/images/generations \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"model": "gemini-3-pro-image", "prompt": "A serene lake at dawn"}'
# Expected: 200 OK, valid base64 image data

# 2. Cache hit (repeat same request)
# Expected: Faster response (~8ms vs 3-5s), cache hit logged

# 3. Safety filtering (if MEDIUM/HIGH)
curl -X POST http://localhost:8045/v1/images/generations \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"model": "gemini-3-pro-image", "prompt": "violent content test"}'
# Expected: May be blocked depending on threshold

# 4. Error logging
grep "error_type" ~/.antigravity_tools/logs/antigravity.log | tail -5
# Expected: Structured error logs with all required fields
```

**Performance Checks** (10-15 minutes):
```bash
# 1. Cache performance
# Generate 10 unique images
for i in {1..10}; do
  curl -X POST http://localhost:8045/v1/images/generations \
    -H "Authorization: Bearer $KEY" \
    -d "{\"model\": \"gemini-3-pro-image\", \"prompt\": \"Test $i\"}"
done

# Repeat 1 image (test cache hit)
curl -X POST http://localhost:8045/v1/images/generations \
  -H "Authorization: Bearer $KEY" \
  -d '{"model": "gemini-3-pro-image", "prompt": "Test 1"}'

# 2. Check cache metrics
ls -lh ~/.antigravity_tools/image_cache/
# Expected: 10 cache files, total size ~4-8MB

# 3. Monitor generation times
grep "generation_time_ms" ~/.antigravity_tools/logs/antigravity.log | \
  awk -F'generation_time_ms=' '{print $2}' | \
  awk '{print $1}' | \
  sort -n | \
  tail -10
# Expected: <5000ms for most requests, <10ms for cache hits
```

---

## Monitoring & Alerting

### Key Metrics

**Business Metrics**:
| Metric | Target | Critical Threshold | Source |
|--------|--------|-------------------|--------|
| Image generation success rate | ≥95% | <90% | Logs (error_type) |
| Average generation time | <5s | >10s | Logs (generation_time_ms) |
| Cache hit rate | ≥30% | <20% | Cache stats |
| Safety rejection rate | 5-15% (MEDIUM) | >30% | Logs (IMG_SAFETY_BLOCKED) |

**Technical Metrics**:
| Metric | Target | Critical Threshold | Source |
|--------|--------|-------------------|--------|
| Error rate | <5% | >10% | Logs (error_type) |
| Cache storage usage | <80% | >95% | Filesystem |
| Proxy uptime | 99.9% | <99% | Process monitor |
| Account rotation health | All accounts active | >50% blocked | Account status |

### Log Queries

**Find quota exhaustion errors**:
```bash
grep "error_type.*API_ERROR" ~/.antigravity_tools/logs/antigravity.log | \
  grep "quota" | \
  tail -20
```

**Find slow generation requests** (>10 seconds):
```bash
grep "generation_time_ms" ~/.antigravity_tools/logs/antigravity.log | \
  awk -F'generation_time_ms=' '{print $2}' | \
  awk '{if ($1 > 10000) print}' | \
  wc -l
```

**Calculate cache hit rate**:
```bash
# Count cache hits
HITS=$(grep "Cache hit" ~/.antigravity_tools/logs/antigravity.log | wc -l)

# Count cache misses
MISSES=$(grep "Cache miss" ~/.antigravity_tools/logs/antigravity.log | wc -l)

# Calculate hit rate
echo "scale=2; $HITS / ($HITS + $MISSES) * 100" | bc
# Expected: ≥30%
```

**Find safety-blocked requests**:
```bash
grep "IMG_SAFETY_BLOCKED" ~/.antigravity_tools/logs/antigravity.log | \
  tail -10
```

**Monitor error distribution**:
```bash
grep "error_type" ~/.antigravity_tools/logs/antigravity.log | \
  awk -F'error_type=' '{print $2}' | \
  awk '{print $1}' | \
  sort | \
  uniq -c | \
  sort -rn
# Expected distribution:
# API_ERROR > USER_ERROR > NETWORK_ERROR > SYSTEM_ERROR
```

### Alert Rules

**Critical Alerts** (page on-call immediately):
- Error rate > 10% for 5 minutes
- Proxy process crash/restart
- Cache corruption detected
- All accounts quota exhausted
- Performance degradation >50% for 15 minutes

**Warning Alerts** (investigate within 1 hour):
- Error rate > 5% for 10 minutes
- Cache hit rate < 20% for 30 minutes
- Safety rejection rate > 30% for 15 minutes
- Cache storage > 90% capacity
- Generation times > 10s for >20% of requests

**Info Alerts** (review during business hours):
- Cache storage > 80% capacity
- Safety rejection rate changed by >10%
- Unusual prompt patterns detected
- Account rotation imbalance

### Dashboard Recommendations

**Real-Time Dashboard** (refresh every 1 minute):
- Current error rate (%)
- Average generation time (ms)
- Cache hit rate (%)
- Active requests (count)
- Proxy status (up/down)

**Historical Dashboard** (hourly/daily views):
- Request volume over time
- Error rate trends
- Cache performance trends
- Safety rejection trends
- Cost savings from cache (estimated)

**Performance Dashboard**:
- p50, p95, p99 generation times
- Cache hit latency histogram
- Generation time by model variant
- Quota consumption by account

---

## Common Issues & Resolutions

### Issue 1: Cache Not Working

**Symptoms**:
- All requests show cache miss
- `Cache hit` never appears in logs
- Generation times consistently high

**Diagnosis**:
```bash
# Check environment variable
echo $CACHE_BACKEND
# Should be: filesystem

# Check cache directory
ls -la ~/.antigravity_tools/image_cache/
# Should exist and be writable

# Check logs for cache errors
grep "cache" ~/.antigravity_tools/logs/antigravity.log | tail -20
```

**Resolution**:
```bash
# 1. Verify CACHE_BACKEND is set
export CACHE_BACKEND=filesystem

# 2. Ensure cache directory exists
mkdir -p ~/.antigravity_tools/image_cache
chmod 755 ~/.antigravity_tools/image_cache

# 3. Restart proxy
# (via UI or restart application)

# 4. Test cache
# Generate same image twice, second should be cache hit
```

**Prevention**:
- Include cache configuration in deployment checklist
- Monitor cache initialization at startup
- Alert on cache backend = none in production

---

### Issue 2: High Safety Rejection Rate

**Symptoms**:
- >30% of requests blocked by safety filter
- Frequent `IMG_SAFETY_BLOCKED` errors
- User complaints about content filtering

**Diagnosis**:
```bash
# Check current threshold
echo $GEMINI_IMAGE_SAFETY_THRESHOLD

# Count blocked requests
grep "IMG_SAFETY_BLOCKED" ~/.antigravity_tools/logs/antigravity.log | wc -l

# Review blocked prompts (hashed)
grep "IMG_SAFETY_BLOCKED" ~/.antigravity_tools/logs/antigravity.log | \
  grep "prompt_hash" | \
  tail -10
```

**Resolution**:
```bash
# Option 1: Lower global threshold
export GEMINI_IMAGE_SAFETY_THRESHOLD=LOW  # Was: MEDIUM

# Option 2: Document per-request override for users
# Users can pass safety_threshold parameter in request

# Option 3: Review specific categories
# Determine which harm categories are triggering most blocks

# Restart proxy with new configuration
```

**Prevention**:
- Start with MEDIUM, adjust based on actual rejection rate
- Educate users about content policies
- Provide clear error messages with resolution steps
- Monitor rejection rate trends

---

### Issue 3: Slow Generation Times

**Symptoms**:
- `generation_time_ms` > 10 seconds consistently
- User complaints about slowness
- Timeout errors

**Diagnosis**:
```bash
# Check average generation time
grep "generation_time_ms" ~/.antigravity_tools/logs/antigravity.log | \
  awk -F'generation_time_ms=' '{print $2}' | \
  awk '{sum+=$1; count++} END {print sum/count}'

# Check cache effectiveness
grep "Cache hit" ~/.antigravity_tools/logs/antigravity.log | wc -l
grep "Cache miss" ~/.antigravity_tools/logs/antigravity.log | wc -l

# Check network connectivity
ping generativelanguage.googleapis.com
```

**Resolution**:
```bash
# 1. Enable/verify cache is working
export CACHE_BACKEND=filesystem

# 2. Check account rotation
# Ensure multiple accounts configured
# Verify accounts not all quota-exhausted

# 3. Increase cache size if hit rate is good but size limit reached
export CACHE_MAX_SIZE_MB=1000  # Was: 500

# 4. Check network/DNS issues
# Verify generativelanguage.googleapis.com resolves
# Check firewall rules

# 5. Review upstream API status
# Check Google Cloud Status Dashboard
```

**Prevention**:
- Monitor p95/p99 generation times
- Alert on performance degradation
- Maintain multiple accounts for load distribution
- Enable caching in production

---

### Issue 4: Cache Growing Too Large

**Symptoms**:
- Cache directory size > `CACHE_MAX_SIZE_MB`
- Disk space warnings
- LRU eviction not working

**Diagnosis**:
```bash
# Check current cache size
du -sh ~/.antigravity_tools/image_cache/

# Check configured limit
echo $CACHE_MAX_SIZE_MB

# Count cache entries
ls ~/.antigravity_tools/image_cache/*.json | wc -l

# Check for eviction logs
grep "Cache eviction" ~/.antigravity_tools/logs/antigravity.log
```

**Resolution**:
```bash
# 1. Manual cleanup (safe - cache rebuilds)
rm -rf ~/.antigravity_tools/image_cache/*

# 2. Restart proxy (to reset cache state)

# 3. If persistent issue, increase limit or reduce TTL
export CACHE_MAX_SIZE_MB=1000  # Increase limit
# OR
export CACHE_TTL_SECONDS=1800  # Shorter TTL (30 min)

# 4. Monitor for LRU eviction working
# After fix, cache should stay under limit
```

**Prevention**:
- Set cache size limit appropriate for available storage
- Alert at 80% cache capacity
- Monitor eviction logs to ensure LRU working
- Consider shorter TTL for high-volume environments

---

### Issue 5: All Accounts Quota Exhausted

**Symptoms**:
- `IMG_QUOTA_EXHAUSTED` errors for all accounts
- All generation requests failing
- `error_type=API_ERROR` with quota messages

**Diagnosis**:
```bash
# Check quota errors
grep "IMG_QUOTA_EXHAUSTED" ~/.antigravity_tools/logs/antigravity.log | tail -20

# Count accounts in quota exhaustion state
grep "quota" ~/.antigravity_tools/logs/antigravity.log | \
  grep -oP 'account_email=\S+' | \
  sort -u | \
  wc -l

# Check account rotation
# (via UI or account list command)
```

**Resolution**:
```bash
# 1. Wait for quota reset (usually midnight PST)
# 2. Add more accounts if available
# 3. Enable caching to reduce quota consumption
export CACHE_BACKEND=filesystem

# 4. Implement rate limiting on client side
# 5. Notify users about temporary service degradation

# 6. Monitor quota reset times
# Google quotas typically reset at midnight Pacific Time
```

**Prevention**:
- Configure multiple accounts (5-10 recommended)
- Enable caching to reduce live API calls by 30-90%
- Monitor quota consumption trends
- Alert when quota reaches 80% on any account
- Implement client-side rate limiting

---

## Cache Management

### Routine Cache Maintenance

**Daily Tasks** (automated):
```bash
# Check cache size
du -sh ~/.antigravity_tools/image_cache/

# Verify cache within limits
# LRU eviction should keep it under CACHE_MAX_SIZE_MB
```

**Weekly Tasks**:
```bash
# Review cache hit rate trends
# Target: ≥30%, investigate if dropping

# Analyze cached prompts (hashed)
# Identify popular prompts for optimization

# Check cache directory health
# Verify no corruption, proper permissions
```

**Monthly Tasks**:
```bash
# Clear cache for fresh start (optional)
rm -rf ~/.antigravity_tools/image_cache/*

# Review cache configuration
# Adjust CACHE_MAX_SIZE_MB or CACHE_TTL_SECONDS based on usage

# Archive old logs
tar -czf logs-archive-$(date +%Y%m).tar.gz \
  ~/.antigravity_tools/logs/*.log.old
```

### Cache Troubleshooting Commands

**Verify cache is enabled**:
```bash
grep "Cache backend:" ~/.antigravity_tools/logs/antigravity.log | head -1
# Expected: "Cache backend: filesystem"
```

**Count cache entries**:
```bash
ls ~/.antigravity_tools/image_cache/*.json | wc -l
```

**Find oldest cache entry**:
```bash
ls -lt ~/.antigravity_tools/image_cache/*.json | tail -1
```

**Find newest cache entry**:
```bash
ls -lt ~/.antigravity_tools/image_cache/*.json | head -1
```

**Calculate cache storage efficiency**:
```bash
# Total size
SIZE=$(du -sm ~/.antigravity_tools/image_cache/ | awk '{print $1}')

# Entry count
COUNT=$(ls ~/.antigravity_tools/image_cache/*.json | wc -l)

# Average per image
echo "scale=2; $SIZE / $COUNT" | bc
# Expected: ~0.4-1.2 MB per image
```

---

## Performance Troubleshooting

### Performance Degradation Checklist

**When performance drops >20%**:
1. [ ] Check cache hit rate (should be ≥30%)
2. [ ] Verify network connectivity to Google APIs
3. [ ] Review account rotation (ensure not all quota-exhausted)
4. [ ] Check system resources (CPU, memory, disk I/O)
5. [ ] Analyze error rate (high errors = slow retries)
6. [ ] Review recent configuration changes
7. [ ] Check Google Cloud Status Dashboard for outages

### Performance Optimization Playbook

**Scenario 1: Cache Hit Rate < 20%**
```bash
# Increase cache size
export CACHE_MAX_SIZE_MB=1000  # Was: 500

# Increase TTL
export CACHE_TTL_SECONDS=14400  # 4 hours (was: 2 hours)

# Restart proxy
```

**Scenario 2: High Request Volume**
```bash
# Add more accounts for load distribution
# (via UI: Settings → Accounts → Add Account)

# Increase cache size
export CACHE_MAX_SIZE_MB=2000

# Enable all performance optimizations
export CACHE_BACKEND=filesystem
export CACHE_TTL_SECONDS=7200
```

**Scenario 3: Network Latency**
```bash
# Check DNS resolution time
time nslookup generativelanguage.googleapis.com

# Check HTTPS connection time
time curl -I https://generativelanguage.googleapis.com

# If high latency:
# - Check local network
# - Check firewall rules
# - Consider proxy configuration
```

---

## Security Incident Response

### Suspected API Key Leak

**Immediate Actions** (within 5 minutes):
1. Rotate all affected API keys
2. Revoke compromised keys in Google Cloud Console
3. Review access logs for unauthorized usage
4. Notify security team

**Investigation** (within 1 hour):
```bash
# Review recent account activity
grep "account_email" ~/.antigravity_tools/logs/antigravity.log | tail -100

# Check for unusual patterns
grep "error_type" ~/.antigravity_tools/logs/antigravity.log | \
  awk -F'prompt_hash=' '{print $2}' | \
  awk '{print $1}' | \
  sort | \
  uniq -c | \
  sort -rn | \
  head -20
# Look for repeated prompt hashes (potential abuse)
```

**Remediation**:
- Change all API keys
- Review and update access controls
- Audit all accounts and quotas
- Document incident for post-mortem

### Suspicious Content Generation

**Detection**:
```bash
# High safety rejection rate (sudden spike)
grep "IMG_SAFETY_BLOCKED" ~/.antigravity_tools/logs/antigravity.log | \
  wc -l

# Unusual error patterns
grep "error_type" ~/.antigravity_tools/logs/antigravity.log | \
  tail -50
```

**Response**:
```bash
# 1. Increase safety threshold temporarily
export GEMINI_IMAGE_SAFETY_THRESHOLD=HIGH

# 2. Review blocked prompts (hashed for privacy)
grep "IMG_SAFETY_BLOCKED" ~/.antigravity_tools/logs/antigravity.log | \
  grep "prompt_hash" | \
  tail -20

# 3. Identify source of suspicious requests
# (check account_email, timestamp patterns)

# 4. Block suspicious accounts if needed
# (via UI or database update)
```

---

## Rollback Procedures

### Rollback Decision Criteria

Roll back deployment if:
- Error rate > 10% for 5+ minutes
- Cache failure impacting >50% of requests
- Performance degradation > 50% sustained
- Critical security vulnerability discovered
- Safety filter malfunction (blocking all or none)

### Rollback Steps

**Quick Rollback** (5-10 minutes):
```bash
# 1. Stop current application
# (via UI or process manager)

# 2. Restore previous configuration
cp ~/.antigravity_tools/config.json.backup \
   ~/.antigravity_tools/config.json

# 3. Restart with previous version
# Install previous .dmg or binary

# 4. Verify functionality
curl http://localhost:8045/v1/images/generations \
  -H "Authorization: Bearer $KEY" \
  -d '{"model": "gemini-3-pro-image", "prompt": "Test"}'

# 5. Communicate status
# Notify team, update status page
```

**Full Rollback** (15-20 minutes):
```bash
# 1. Document rollback reason
# Create incident report

# 2. Restore backup
tar -xzf antigravity-backup-YYYYMMDD.tar.gz -C ~/

# 3. Restart application with previous version

# 4. Run smoke tests
# Verify all critical paths working

# 5. Post-rollback monitoring
# Watch for 1 hour to ensure stability

# 6. Root cause analysis
# Investigate what went wrong
# Update deployment procedures
```

---

## Maintenance Tasks

### Weekly Maintenance

**Every Monday** (15 minutes):
```bash
# 1. Review error logs
grep "error_type" ~/.antigravity_tools/logs/antigravity.log | \
  tail -100 > weekly-errors-$(date +%Y%m%d).txt

# 2. Check cache health
du -sh ~/.antigravity_tools/image_cache/
ls ~/.antigravity_tools/image_cache/*.json | wc -l

# 3. Review performance metrics
# Generation time trends
# Cache hit rate trends
# Safety rejection rate

# 4. Update documentation
# Document any new issues encountered
# Update runbook with lessons learned
```

### Monthly Maintenance

**First Sunday of each month** (30 minutes):
```bash
# 1. Archive old logs
mkdir -p ~/.antigravity_tools/logs/archive
mv ~/.antigravity_tools/logs/*.log.old \
   ~/.antigravity_tools/logs/archive/logs-$(date +%Y%m).tar.gz

# 2. Clear cache for fresh start
rm -rf ~/.antigravity_tools/image_cache/*

# 3. Review account health
# Check quota usage patterns
# Rotate accounts if needed
# Verify all accounts active

# 4. Update configuration if needed
# Based on usage trends from past month

# 5. Security audit
# Review access logs
# Check for anomalies
# Update API keys if needed

# 6. Performance review
# Compare metrics month-over-month
# Identify optimization opportunities
```

### Quarterly Maintenance

**Every quarter** (1-2 hours):
```bash
# 1. Full backup
tar -czf antigravity-full-backup-$(date +%Y%m%d).tar.gz \
  ~/.antigravity_tools/

# 2. Configuration review
# Review all environment variables
# Update based on lessons learned
# Document configuration rationale

# 3. Disaster recovery test
# Test restore from backup
# Verify all functionality

# 4. Performance baseline
# Establish new performance baseline
# Compare to previous quarter
# Document trends

# 5. Security review
# Review all access controls
# Audit API key usage
# Update security procedures

# 6. Documentation update
# Update this runbook
# Update configuration guide
# Document new patterns/issues
```

---

## Appendix

### Emergency Contacts

- **On-Call Engineer**: (configured in monitoring system)
- **Tech Lead**: (Story-007-05 Tech Lead)
- **Product Owner**: (Epic-007 stakeholder)

### External Resources

- **Google Cloud Status**: https://status.cloud.google.com/
- **Gemini API Documentation**: https://ai.google.dev/
- **Antigravity Tools Repository**: (internal link)

### Related Documentation

- **Configuration Guide**: `docs/configuration/image-generation.md`
- **Workflow Documentation**: `docs/antigravity/workflows/models/gemini/gemini-3-pro-image-workflow.md`
- **Testing Guide**: `docs/testing/image-generation-tests.md`
- **Troubleshooting Guide**: `docs/troubleshooting/image-generation-errors.md`

---

**Runbook Version**: 1.0
**Epic**: Epic-007
**Status**: Production Ready
**Last Updated**: 2026-01-11
**Next Review**: 2026-02-11
