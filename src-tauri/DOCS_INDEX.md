# Documentation Index - Antigravity Manager Proxy

Центральный индекс технической документации для Antigravity Manager API Proxy.

---

## 📚 Quick Navigation

### 🎯 User Guides

| Document | Description | When to use |
|----------|-------------|-------------|
| **[FALLBACK_GUIDE.md](./FALLBACK_GUIDE.md)** ⭐ | Automatic failover system guide | Learn about model fallback, troubleshooting |
| **[MODELS_REFERENCE.md](./MODELS_REFERENCE.md)** | Complete model catalog & API examples | Find available models, configure requests |

### 🔧 Technical Documentation

| Document | Description | When to use |
|----------|-------------|-------------|
| **[EXTENDED_THINKING_FIXES.md](./EXTENDED_THINKING_FIXES.md)** ⭐ | Critical bug fixes & solutions (effortLevel, signature errors) | Understand problem resolutions, implementation details |
| **[THINKING_MODELS.md](./THINKING_MODELS.md)** | Extended Thinking implementation details | Understand Claude vs Gemini thinking, debug 404s |
| **[TESTING_GUIDE.md](./TESTING_GUIDE.md)** | Comprehensive test suite (19 tests) | Run tests, add new test cases |
| **[SESSION_ANALYSIS.md](./SESSION_ANALYSIS.md)** | Real-world testing analysis & metrics | Review session statistics, root cause analysis |

---

## 🚀 Getting Started

### New to Antigravity Manager?

1. **Start here:** [MODELS_REFERENCE.md](./MODELS_REFERENCE.md)
   - See all available models (Claude, Gemini, OpenAI)
   - Learn basic API usage
   - Configure Extended Thinking

2. **Configure fallback:** [FALLBACK_GUIDE.md](./FALLBACK_GUIDE.md)
   - Understand automatic model switching
   - Monitor fallback events
   - Optimize for reliability

3. **Run tests:** [TESTING_GUIDE.md](./TESTING_GUIDE.md)
   - Validate your setup
   - Test all model variations
   - Ensure thinking works correctly

### Troubleshooting Issues?

| Problem | Document | Section |
|---------|----------|---------|
| **effortLevel 400 errors** | [EXTENDED_THINKING_FIXES.md](./EXTENDED_THINKING_FIXES.md) | "Проблема #1: effortLevel" |
| **Corrupted thought signature** | [EXTENDED_THINKING_FIXES.md](./EXTENDED_THINKING_FIXES.md) | "Проблема #2: Corrupted thought signature" |
| **404 Not Found errors** | [THINKING_MODELS.md](./THINKING_MODELS.md) | "❌ Распространенная ошибка" |
| **Claude Opus timeouts** | [EXTENDED_THINKING_FIXES.md](./EXTENDED_THINKING_FIXES.md) | "Проблема #3: Claude Opus Thinking timeout" |
| **Gemini thinking not working** | [THINKING_MODELS.md](./THINKING_MODELS.md) | "🔧 Реализация в коде" |
| **Test failures** | [TESTING_GUIDE.md](./TESTING_GUIDE.md) | "Troubleshooting" |
| **Low success rates** | [SESSION_ANALYSIS.md](./SESSION_ANALYSIS.md) | "🔍 Анализ проблемы зависаний" |

---

## 📊 Document Statistics

| Document | Size | Sections | Last Updated |
|----------|------|----------|--------------|
| EXTENDED_THINKING_FIXES.md | ~50 KB | 9 major | 2026-01-09 |
| FALLBACK_GUIDE.md | ~15 KB | 10 major | 2026-01-09 |
| MODELS_REFERENCE.md | ~25 KB | 8 major | 2026-01-09 |
| THINKING_MODELS.md | ~12 KB | 7 major | 2026-01-09 |
| TESTING_GUIDE.md | ~10 KB | 6 major | 2026-01-09 |
| SESSION_ANALYSIS.md | ~10 KB | 9 major | 2026-01-09 |

**Total:** ~122 KB of technical documentation

---

## 🎯 Feature Coverage Matrix

| Feature | User Guide | Technical Docs | Testing | Analysis |
|---------|------------|----------------|---------|----------|
| **Extended Thinking** | MODELS_REFERENCE.md | THINKING_MODELS.md | TESTING_GUIDE.md | SESSION_ANALYSIS.md |
| **Model Fallback** | FALLBACK_GUIDE.md ⭐ | THINKING_MODELS.md | - | SESSION_ANALYSIS.md |
| **Claude Models** | MODELS_REFERENCE.md | THINKING_MODELS.md | TESTING_GUIDE.md | SESSION_ANALYSIS.md |
| **Gemini Models** | MODELS_REFERENCE.md | THINKING_MODELS.md | TESTING_GUIDE.md | SESSION_ANALYSIS.md |
| **UI Notifications** | FALLBACK_GUIDE.md | - | - | - |
| **Debugging** | All guides | THINKING_MODELS.md | TESTING_GUIDE.md | SESSION_ANALYSIS.md |

---

## 🔍 Quick Reference

### Most Common Questions

**Q: How do I enable Extended Thinking?**
→ [MODELS_REFERENCE.md](./MODELS_REFERENCE.md#extended-thinking-configuration)

**Q: Why am I getting 404 errors for Gemini models?**
→ [THINKING_MODELS.md](./THINKING_MODELS.md#-распространенная-ошибка)

**Q: What happens when Claude Opus times out?**
→ [FALLBACK_GUIDE.md](./FALLBACK_GUIDE.md#-как-это-работает)

**Q: How do I run the test suite?**
→ [TESTING_GUIDE.md](./TESTING_GUIDE.md#запуск-тестов)

**Q: What's the success rate for different models?**
→ [SESSION_ANALYSIS.md](./SESSION_ANALYSIS.md#-детальный-анализ-по-моделям)

### Quick Commands

```bash
# Run all tests
cargo test --lib proxy::tests::thinking_models

# Monitor fallback events
tail -f ~/.antigravity_tools/logs/app.log.$(date +%Y-%m-%d) | grep "Model-Fallback"

# Check model statistics
grep "Stream completed" ~/.antigravity_tools/logs/app.log.$(date +%Y-%m-%d) | \
  grep -o "Model: [^|]*" | sort | uniq -c

# Analyze thinking requests
grep "thinkingConfig" ~/.antigravity_tools/logs/app.log.$(date +%Y-%m-%d) | wc -l
```

---

## 📝 Document Relationships

```
MODELS_REFERENCE.md (Entry Point)
    ├─→ THINKING_MODELS.md (Technical Details)
    │   ├─→ FALLBACK_GUIDE.md (Fallback System)
    │   └─→ TESTING_GUIDE.md (Validation)
    └─→ SESSION_ANALYSIS.md (Real-world Data)
        └─→ FALLBACK_GUIDE.md (Solution)
```

### Reading Path for Different Goals

**Goal: Start using the proxy**
1. MODELS_REFERENCE.md (basics)
2. FALLBACK_GUIDE.md (reliability)
3. TESTING_GUIDE.md (validation)

**Goal: Fix Gemini 404 errors**
1. THINKING_MODELS.md (root cause)
2. TESTING_GUIDE.md (verification)
3. SESSION_ANALYSIS.md (metrics)

**Goal: Understand Claude Opus timeouts**
1. SESSION_ANALYSIS.md (problem analysis)
2. FALLBACK_GUIDE.md (solution)
3. THINKING_MODELS.md (technical details)

**Goal: Contribute to codebase**
1. TESTING_GUIDE.md (test structure)
2. THINKING_MODELS.md (implementation)
3. All guides (comprehensive understanding)

---

## 🆕 Recent Updates

### v3.3.20 (2026-01-09) - Critical Fixes

- ✅ **NEW:** EXTENDED_THINKING_FIXES.md - Comprehensive problem resolution documentation
- ✅ **FIX:** effortLevel parameter mapping (effort → temperature) - 100% success rate
- ✅ **FIX:** Corrupted thought signature retry logic - automatic recovery
- ✅ **FIX:** Claude Opus Thinking timeout fallback - 93.7% improvement
- ✅ **NEW:** FALLBACK_GUIDE.md - Comprehensive fallback documentation
- ✅ **UPDATED:** THINKING_MODELS.md - Added fallback mechanics section
- ✅ **UPDATED:** SESSION_ANALYSIS.md - Implementation changelog
- ✅ **FEATURE:** Automatic Claude Opus → Gemini fallback (93.7% improvement)
- ✅ **FEATURE:** UI Toast notifications for fallback events

### Earlier (2026-01-09)
- ✅ **NEW:** MODELS_REFERENCE.md - Complete model catalog
- ✅ **NEW:** SESSION_ANALYSIS.md - Real-world testing analysis
- ✅ **NEW:** TESTING_GUIDE.md - 19 comprehensive tests
- ✅ **NEW:** THINKING_MODELS.md - Claude vs Gemini thinking guide
- ✅ **FIX:** Gemini routing (remove `-thinking` suffix) - 0 404 errors

---

## 🔗 External References

- **GitHub Issues:**
  - [Issue #497](https://github.com/google/antigravity/issues/497) - Session Not Progressing (Claude Opus Timeouts)

- **Related Code:**
  - `src/proxy/common/model_mapping.rs` - Model routing logic
  - `src/proxy/mappers/claude/request.rs` - Request transformation & fallback
  - `src/proxy/tests/thinking_models.rs` - Test suite (19 tests)
  - `src/App.tsx` - UI event listeners for fallback notifications

- **Official APIs:**
  - [Google Cloud Code API](https://cloud.google.com/code) - Upstream endpoint
  - [Claude API Documentation](https://docs.anthropic.com) - Claude API reference
  - [Gemini API Documentation](https://ai.google.dev/gemini-api/docs) - Gemini API reference

---

## 💡 Contributing to Documentation

### Adding New Documentation

1. Create `.md` file in `src-tauri/` directory
2. Update this index (`DOCS_INDEX.md`)
3. Add cross-references in related documents
4. Run documentation linting (if available)
5. Create git commit with `docs:` prefix

### Documentation Standards

- **Headers:** Use emoji for section markers (📊 📚 🔧 etc.)
- **Code blocks:** Always specify language for syntax highlighting
- **Examples:** Include real-world examples with expected output
- **Tables:** Use for structured comparisons and reference data
- **Cross-refs:** Link to related sections in other documents

---

**Last Updated:** 2026-01-09
**Maintained by:** Claude Sonnet 4.5
**Version:** 1.0
