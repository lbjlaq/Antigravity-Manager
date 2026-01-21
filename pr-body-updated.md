## Problems Fixed

### 1. Non-deterministic Wildcard Matching
When multiple wildcard rules match a model request, HashMap iteration order caused unpredictable routing behavior.

**Example scenario:**
- Rules: `gpt*` → `fallback`, `gpt-4*` → `specific`
- Request: `gpt-4-turbo`
- **Before**: Random result (could be either)
- **After**: Always routes to `specific` (higher specificity)

### 2. Multi-Wildcard Bug (Copilot AI Review)
`wildcard_match` only supported single wildcard via `pattern.find('*')`, but `specificity` counted all wildcards via `pattern.matches('*').count()`, causing inconsistency.

**Solution**: Rewrote `wildcard_match` to support **multiple wildcards** with proper glob-style matching.

---

## Features

### Most-Specific-Match Strategy
Selects the rule with highest specificity (most non-wildcard characters).

**Specificity = pattern.len() - wildcard_count**

| Rule | Specificity | Request `claude-opus-4-5-thinking` |
|------|-------------|------------------------------------|
| `claude-opus*thinking` | 20 | ✓ Selected |
| `claude-opus-*` | 12 | - |

### Multi-Wildcard Support (NEW)
- ✅ `claude-*-sonnet-*` matches `claude-3-5-sonnet-20241022`
- ✅ `gpt-*-*` matches `gpt-4-turbo-preview`
- ✅ `*thinking*` matches any model containing "thinking"
- ✅ Single-wildcard patterns remain fully compatible

---

## Changes
- Rewrote `wildcard_match` function with multi-wildcard support
- Modified `resolve_model_route` with deterministic priority logic
- Added comprehensive tests: `test_wildcard_priority`, `test_multi_wildcard_support`, `test_wildcard_edge_cases`

---

## Testing

```bash
cd src-tauri
cargo test model_mapping
```

**Result**: All 4 tests passed ✅
```
test proxy::common::model_mapping::tests::test_model_mapping ... ok
test proxy::common::model_mapping::tests::test_wildcard_priority ... ok
test proxy::common::model_mapping::tests::test_multi_wildcard_support ... ok
test proxy::common::model_mapping::tests::test_wildcard_edge_cases ... ok

test result: ok. 4 passed; 0 failed; 0 ignored
```

---

## Impact Assessment

| Aspect | Impact |
|--------|--------|
| **Breaking Changes** | None - single-wildcard patterns fully compatible |
| **Performance** | Negligible (pattern lists typically <100 rules) |
| **Complexity** | Medium - comprehensive test coverage added |
| **Benefits** | Deterministic routing + flexible multi-wildcard support |
