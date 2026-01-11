# Image Generation Configuration Guide

**Epic**: Epic-007 - Gemini 3 Pro Image Compliance
**Version**: v3.3.20
**Last Updated**: 2026-01-11
**Status**: Production Ready

---

## Overview

Comprehensive configuration guide for Gemini 3 Pro Image generation in Antigravity Tools. This guide covers environment variables, safety settings, caching, and performance tuning for all 4 implemented stories (007-01 through 007-04).

---

## Environment Variables

### Safety Settings (Story-007-02)

Control content filtering for all image generation requests.

| Variable | Values | Default | Description |
|----------|--------|---------|-------------|
| `GEMINI_IMAGE_SAFETY_THRESHOLD` | `OFF`, `LOW`, `MEDIUM`, `HIGH` | `OFF` | Global safety filter level |

**Gemini API Mapping**:
- `OFF` → No filtering (backward compatible)
- `LOW` → `BLOCK_ONLY_HIGH` (blocks only high-severity content)
- `MEDIUM` → `BLOCK_MEDIUM_AND_ABOVE` (blocks medium+ severity)
- `HIGH` → `BLOCK_LOW_AND_ABOVE` (blocks low+ severity, strictest)

**Setting the Variable**:
```bash
# Development
export GEMINI_IMAGE_SAFETY_THRESHOLD=OFF

# Production (balanced)
export GEMINI_IMAGE_SAFETY_THRESHOLD=MEDIUM

# High-compliance environment
export GEMINI_IMAGE_SAFETY_THRESHOLD=HIGH
```

**Filtered Categories**:
1. **HARM_CATEGORY_HATE_SPEECH**: Hate speech and discrimination
2. **HARM_CATEGORY_DANGEROUS_CONTENT**: Dangerous or violent content
3. **HARM_CATEGORY_HARASSMENT**: Harassment and bullying
4. **HARM_CATEGORY_SEXUALLY_EXPLICIT**: Sexually explicit content

### Caching Configuration (Story-007-04)

Control response caching to reduce costs and improve performance.

| Variable | Values | Default | Description |
|----------|--------|---------|-------------|
| `CACHE_BACKEND` | `none`, `filesystem`, `redis` | `none` | Cache backend type |
| `CACHE_MAX_SIZE_MB` | `1-10000` | `100` | Maximum cache size (MB) |
| `CACHE_TTL_SECONDS` | `60-86400` | `3600` | Cache entry TTL (1 hour default) |
| `CACHE_DIR` | `path` | `{data_dir}/image_cache/` | Cache storage directory |

**Cache Backend Options**:
- `none`: No caching (testing, compliance requirements)
- `filesystem`: Production-ready file-based cache (recommended)
- `redis`: Reserved for future implementation

**Platform-Specific Cache Directories**:
- **macOS**: `~/Library/Application Support/com.lbjlaq.antigravity-tools/image_cache/`
- **Windows**: `%APPDATA%\com.lbjlaq.antigravity-tools\image_cache\`
- **Linux**: `~/.config/com.lbjlaq.antigravity-tools/image_cache/`

**Setting Cache Variables**:
```bash
# Enable filesystem cache (production)
export CACHE_BACKEND=filesystem
export CACHE_MAX_SIZE_MB=500
export CACHE_TTL_SECONDS=7200  # 2 hours

# Development (smaller cache, shorter TTL)
export CACHE_BACKEND=filesystem
export CACHE_MAX_SIZE_MB=50
export CACHE_TTL_SECONDS=1800  # 30 minutes

# Compliance mode (no caching)
export CACHE_BACKEND=none
```

---

## Configuration Profiles

### Development Environment

**Purpose**: Fast iteration, debugging, quota conservation

```bash
# Safety: Off (allow all content for testing)
export GEMINI_IMAGE_SAFETY_THRESHOLD=OFF

# Cache: Enabled with short TTL
export CACHE_BACKEND=filesystem
export CACHE_MAX_SIZE_MB=50
export CACHE_TTL_SECONDS=1800  # 30 minutes
```

**Characteristics**:
- No content filtering (test all prompts)
- Short cache TTL for rapid iteration
- Smaller cache size (development machine)
- E2E tests use mocked data (zero quota)

### Production Environment

**Purpose**: High performance, cost optimization, balanced safety

```bash
# Safety: Medium (balanced filtering)
export GEMINI_IMAGE_SAFETY_THRESHOLD=MEDIUM

# Cache: Large size, longer TTL
export CACHE_BACKEND=filesystem
export CACHE_MAX_SIZE_MB=500
export CACHE_TTL_SECONDS=7200  # 2 hours
```

**Characteristics**:
- Medium safety filtering (blocks medium+ content)
- Large cache for high traffic
- 2-hour TTL balances freshness vs hits
- Estimated 30-90% cost reduction with cache

### High-Compliance Environment

**Purpose**: Strict content filtering, privacy compliance, auditing

```bash
# Safety: High (strictest filtering)
export GEMINI_IMAGE_SAFETY_THRESHOLD=HIGH

# Cache: Disabled (compliance requirement)
export CACHE_BACKEND=none
```

**Characteristics**:
- Strictest content filtering (blocks low+ severity)
- No caching (for privacy/compliance)
- All requests generate fresh images
- Higher API costs (no cache savings)

### Testing/CI Environment

**Purpose**: Automated testing, zero quota consumption

```bash
# Safety: Off (allow test content)
export GEMINI_IMAGE_SAFETY_THRESHOLD=OFF

# Cache: Disabled (test isolation)
export CACHE_BACKEND=none

# API Key: Mock key for structure validation
export ANTIGRAVITY_API_KEY="test-key-for-ci"
```

**Characteristics**:
- E2E tests run with mocked responses
- Zero live API calls (quota protected)
- Fast test execution (<2 seconds)
- Validates code structure only

---

## Per-Request Configuration

### Safety Threshold Override

Request-level safety settings override environment variable.

**JSON Request** (`/v1/images/generations`):
```json
{
  "model": "gemini-3-pro-image-4k-16x9",
  "prompt": "A beautiful sunset over mountains",
  "safety_threshold": "HIGH"
}
```

**Multipart Request** (`/v1/images/edits`):
```bash
curl -X POST http://localhost:8045/v1/images/edits \
  -F "image=@base.png" \
  -F "prompt=Add a rainbow" \
  -F "safety_threshold=MEDIUM"
```

**Priority Order**:
1. Request-level `safety_threshold` parameter
2. Environment variable `GEMINI_IMAGE_SAFETY_THRESHOLD`
3. Default value (`OFF` for backward compatibility)

**Use Cases**:
- **Stricter filtering for user-generated prompts**: Set env to `MEDIUM`, override to `HIGH` for untrusted input
- **Relaxed filtering for internal tools**: Set env to `HIGH`, override to `OFF` for internal testing
- **Dynamic filtering**: Adjust per user tier (free users → `HIGH`, paid users → `MEDIUM`)

---

## Performance Tuning

### Cache Optimization

**Workload Analysis**:

| Workload Type | CACHE_MAX_SIZE_MB | CACHE_TTL_SECONDS | Expected Hit Rate |
|---------------|-------------------|-------------------|-------------------|
| Repetitive prompts (documentation, branding) | 500-1000 | 7200-14400 | 60-90% |
| Diverse prompts (user-generated) | 200-500 | 3600-7200 | 30-50% |
| One-time generation (prototypes) | 50-100 | 1800-3600 | 10-20% |
| Compliance/audit (no caching) | N/A (none) | N/A | 0% |

**Cache Size Guidelines**:
- **Small (50MB)**: ~120 images (standard quality)
- **Medium (100MB)**: ~240 images
- **Large (500MB)**: ~1,200 images
- **Enterprise (1GB)**: ~2,400 images

**Storage Calculation**:
- Standard quality image: ~420KB average
- HD quality image: ~850KB average
- 4K quality image: ~1.2MB average

**TTL Selection**:
- **Short (30 min)**: Rapid iteration, changing requirements
- **Medium (1-2 hours)**: Balanced freshness vs cache hits
- **Long (4-6 hours)**: Stable prompts, high repeat rate
- **Very Long (12-24 hours)**: Branding/logos, rarely change

### Safety vs Performance

Safety filtering adds minimal overhead but may reject some requests.

| Threshold | Filtering Overhead | Rejection Rate | Use Case |
|-----------|-------------------|----------------|----------|
| `OFF` | 0% | 0% | Testing, internal tools |
| `LOW` | ~2% | <5% | Most production use cases |
| `MEDIUM` | ~5% | 5-15% | Balanced safety/usability |
| `HIGH` | ~8% | 15-30% | High-compliance environments |

**Performance Impact**:
- Filtering happens upstream (Gemini API)
- No client-side performance penalty
- Rejected requests don't consume quota
- Error logging adds <1ms overhead (Story-007-03)

### Memory Considerations

**Cache Memory Usage**:
- **Filesystem Cache**: Low memory (<10MB for index)
  - Images stored on disk
  - In-memory index of metadata
  - LRU eviction when size limit exceeded
- **Redis Cache** (future): Higher memory
  - All images in Redis memory
  - Faster access (<1ms vs ~8ms)
  - Requires Redis server

**Application Memory**:
- Base proxy: ~50-100MB
- Per request: ~5-10MB (processing)
- Cache enabled: +10MB (index)
- Total typical: ~70-120MB

---

## Configuration Best Practices

### 1. Start with Defaults, Tune Based on Metrics

```bash
# Week 1: Defaults
export GEMINI_IMAGE_SAFETY_THRESHOLD=MEDIUM
export CACHE_BACKEND=filesystem
export CACHE_MAX_SIZE_MB=100
export CACHE_TTL_SECONDS=3600

# Monitor: Hit rate, rejection rate, storage usage
# Adjust: Increase cache size if hit rate >30% and storage available
```

### 2. Separate Environments

Don't use the same configuration for dev/staging/production.

```bash
# Development
export GEMINI_IMAGE_SAFETY_THRESHOLD=OFF
export CACHE_MAX_SIZE_MB=50

# Staging (production-like)
export GEMINI_IMAGE_SAFETY_THRESHOLD=MEDIUM
export CACHE_MAX_SIZE_MB=200

# Production
export GEMINI_IMAGE_SAFETY_THRESHOLD=MEDIUM
export CACHE_MAX_SIZE_MB=500
```

### 3. Monitor and Alert

Set up monitoring for:
- Cache hit rate (target: ≥30%)
- Safety filter rejection rate (expect: 5-15% for MEDIUM)
- Cache storage usage (alert at 80% capacity)
- Generation errors (Story-007-03 structured logging)

### 4. Cache Warming

For high-traffic prompts, pre-populate cache:

```bash
# Example: Warm cache with common branding images
curl http://localhost:8045/v1/images/generations \
  -H "Authorization: Bearer $KEY" \
  -d '{"model": "gemini-3-pro-image", "prompt": "Company logo on white background"}'
```

### 5. Disaster Recovery

**Cache Failure**:
- Filesystem cache failure → Automatic fallback to no-cache mode
- Requests continue (no downtime)
- Monitor for cache errors in logs

**Full Cache Recovery**:
```bash
# Clear corrupted cache
rm -rf ~/.antigravity_tools/image_cache/*

# Restart application (cache rebuilds automatically)
npm run tauri dev
```

---

## Troubleshooting

### Issue: Cache Not Working

**Symptoms**: All requests are cache misses

**Diagnosis**:
```bash
# Check environment variable
echo $CACHE_BACKEND
# Should output: filesystem

# Check cache directory exists
ls -la ~/.antigravity_tools/image_cache/
```

**Fix**:
```bash
# Set correct backend
export CACHE_BACKEND=filesystem

# Ensure directory is writable
mkdir -p ~/.antigravity_tools/image_cache
chmod 755 ~/.antigravity_tools/image_cache
```

### Issue: Safety Filter Blocking Too Much

**Symptoms**: High rejection rate (>20%)

**Diagnosis**:
```bash
# Check current threshold
echo $GEMINI_IMAGE_SAFETY_THRESHOLD
# Current: HIGH

# Review error logs (Story-007-03)
grep "IMG_SAFETY_BLOCKED" logs/antigravity.log
```

**Fix**:
```bash
# Lower threshold
export GEMINI_IMAGE_SAFETY_THRESHOLD=MEDIUM

# Or disable for testing
export GEMINI_IMAGE_SAFETY_THRESHOLD=OFF

# Restart proxy
```

### Issue: Slow Generation Times

**Symptoms**: `generation_time_ms` > 10 seconds

**Diagnosis**:
```bash
# Check if cache is enabled
echo $CACHE_BACKEND

# Review generation times in logs
grep "generation_time_ms" logs/antigravity.log | awk '{print $NF}' | sort -n
```

**Fix**:
```bash
# Enable cache if not already
export CACHE_BACKEND=filesystem

# Check network connectivity
ping generativelanguage.googleapis.com

# Verify account rotation working
# (multiple accounts should share load)
```

### Issue: Cache Size Growing Too Large

**Symptoms**: Cache directory > `CACHE_MAX_SIZE_MB` limit

**Diagnosis**:
```bash
# Check current cache size
du -sh ~/.antigravity_tools/image_cache/

# Check configured limit
echo $CACHE_MAX_SIZE_MB
```

**Fix**:
```bash
# Manual cleanup (safe - cache rebuilds)
rm -rf ~/.antigravity_tools/image_cache/*

# Or increase limit if storage available
export CACHE_MAX_SIZE_MB=1000

# LRU eviction should prevent unbounded growth
# If not working, report as bug
```

---

## Security Considerations

### API Key Protection

Never log or cache API keys.

```bash
# ✅ GOOD: Environment variable
export ANTIGRAVITY_API_KEY="your-key-here"

# ❌ BAD: Hardcoded in config file (avoid)
# ❌ BAD: Logged to console
```

### Prompt Privacy

Prompts are hashed in logs (Story-007-03) for privacy.

```bash
# Logs contain prompt_hash, not full prompt
grep "prompt_hash" logs/antigravity.log
# Output: prompt_hash=a1b2c3d4e5f6... (16 characters)
```

**Privacy Features**:
- SHA256 hashing (cryptographically secure)
- 16-character hash (collision-resistant)
- Original prompts never logged
- Cache keys also use hashed prompts

### Cache Security

Cached images are stored locally with filesystem permissions.

```bash
# Check cache permissions
ls -la ~/.antigravity_tools/image_cache/

# Should be user-only readable (700 or 755)
# Images are base64-encoded JSON (not browsable)
```

**Recommendations**:
- Use encrypted filesystem for cache directory
- Set restrictive permissions (700)
- Regularly rotate cache (clear old entries)
- Consider `CACHE_BACKEND=none` for sensitive content

---

## Migration Guide

### Upgrading from Epic-006 to Epic-007

Epic-007 adds safety settings and caching. No breaking changes.

**Before Epic-007**:
```bash
# No safety configuration
# No caching
# All requests hit live API
```

**After Epic-007**:
```bash
# Safety configurable (default: OFF, backward compatible)
export GEMINI_IMAGE_SAFETY_THRESHOLD=OFF

# Caching available (default: none, backward compatible)
export CACHE_BACKEND=filesystem
export CACHE_MAX_SIZE_MB=100
export CACHE_TTL_SECONDS=3600
```

**Migration Steps**:
1. Update to v3.3.20
2. (Optional) Enable caching for cost savings
3. (Optional) Set safety threshold for content filtering
4. No code changes required (fully backward compatible)

---

## Configuration Checklist

### Pre-Production Checklist

- [ ] Safety threshold set appropriately (`GEMINI_IMAGE_SAFETY_THRESHOLD`)
- [ ] Caching enabled if desired (`CACHE_BACKEND=filesystem`)
- [ ] Cache size limits configured (`CACHE_MAX_SIZE_MB`)
- [ ] Cache TTL appropriate for workload (`CACHE_TTL_SECONDS`)
- [ ] Cache directory has sufficient storage
- [ ] Monitoring set up for hit rate and errors
- [ ] API keys secured (environment variables, not hardcoded)
- [ ] Test safety filtering with sample prompts
- [ ] Verify cache warming for high-traffic prompts

### Post-Deployment Checklist

- [ ] Monitor cache hit rate (target: ≥30%)
- [ ] Monitor safety rejection rate (expect: 5-15% for MEDIUM)
- [ ] Check cache storage usage (should stay under limit)
- [ ] Review error logs for unexpected issues
- [ ] Validate per-request overrides working
- [ ] Confirm generation times acceptable
- [ ] Test cache eviction under load

---

## References

- **Story-007-01**: E2E Testing Infrastructure
- **Story-007-02**: Safety Settings Configuration
- **Story-007-03**: Enhanced Error Logging
- **Story-007-04**: Response Caching Layer
- **Workflow Documentation**: `docs/antigravity/workflows/models/gemini/gemini-3-pro-image-workflow.md`
- **Operations Runbook**: `docs/operations/image-generation-runbook.md`

---

**Document Version**: 1.0
**Epic**: Epic-007
**Status**: Production Ready
**Last Updated**: 2026-01-11
