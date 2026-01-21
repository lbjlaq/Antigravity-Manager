## Problem
When multiple wildcard rules match a model request, the previous implementation used HashMap iteration (non-deterministic order), causing unpredictable routing behavior.

**Example scenario:**
- Rule 1: `gpt*` → `fallback`
- Rule 2: `gpt-4*` → `specific`
- Request: `gpt-4-turbo`
- Previous: Random result (could be either)
- Now: Always routes to `specific` (higher specificity)

## Solution
Implemented **most-specific-match** strategy: selects the rule with highest specificity (most non-wildcard characters).

**Specificity = pattern.len() - wildcard_count**

| Rule | Specificity | Request `claude-opus-4-5-thinking` |
|------|-------------|------------------------------------|
| `claude-opus*thinking` | 19 | ✓ Selected |
| `claude-opus-*` | 12 | - |

## Changes
- Modified `resolve_model_route` function in `src-tauri/src/proxy/common/model_mapping.rs`
- Added unit test `test_wildcard_priority` to verify behavior
- All existing tests passing

## Testing
```bash
cd src-tauri
cargo test model_mapping
```

**Result:**
```
test proxy::common::model_mapping::tests::test_model_mapping ... ok
test proxy::common::model_mapping::tests::test_wildcard_priority ... ok
```
