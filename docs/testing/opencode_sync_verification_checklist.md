# OpenCode Sync Verification Checklist

> Manual test checklist for OpenCode sync feature PR

## 1. Pre-check

### 1.1 Backup Files Path
- [ ] Verify backup suffix: `.antigravity-manager.bak` (new) and `.antigravity.bak` (legacy)
- [ ] Verify backup location: `~/.config/opencode/opencode.json.antigravity-manager.bak`
- [ ] Verify accounts backup: `~/.config/opencode/antigravity-accounts.json.antigravity-manager.bak`

### 1.2 Plugin Installation Scenarios
| Scenario | Expected Behavior |
|----------|-------------------|
| Plugin NOT installed | Sync button available, shows "OpenCode not detected" warning |
| Plugin installed | Shows version, sync enabled |
| Plugin path auto-detect | Finds opencode in PATH, npm, pnpm, yarn, nvm, fnm, Volta |

---

## 2. Sync Behavior Verification

### 2.1 Provider Creation
- [ ] `provider.antigravity-manager` created with correct structure
- [ ] `npm`: `@ai-sdk/anthropic`
- [ ] `name`: `Antigravity Manager`
- [ ] `options.baseURL`: ends with `/v1` (auto-normalized)
- [ ] `options.apiKey`: matches proxy API key

### 2.2 Existing Providers Not Overwritten
- [ ] `provider.google` preserved (if exists)
- [ ] `provider.anthropic` preserved (if exists)
- [ ] Other providers untouched

### 2.3 Accounts Export File (v3 Structure)
```json
{
  "version": 3,
  "accounts": [...],
  "activeIndex": 0,
  "activeIndexByFamily": {
    "claude": 0,
    "gemini": 0
  }
}
```
- [ ] File created at `~/.config/opencode/antigravity-accounts.json`
- [ ] `version` field = 3
- [ ] `activeIndex` clamped to valid range
- [ ] `activeIndexByFamily` contains `claude` and `gemini` keys
- [ ] Disabled accounts excluded from export

---

## 3. Variants/Thinking Behavior Verification

### 3.1 Claude Thinking Models
```bash
opencode run "test" --model antigravity-manager/claude-sonnet-4-6-thinking --variant high
```
- [ ] `--variant low` → `thinkingBudget: 8192`
- [ ] `--variant medium` → `thinkingBudget: 16384`
- [ ] `--variant high` → `thinkingBudget: 24576`
- [ ] `--variant max` → `thinkingBudget: 32768`

### 3.2 Gemini 3 Pro Models
```bash
opencode run "test" --model antigravity-manager/gemini-3-pro-high --variant low
```
- [ ] `--variant low` → `thinkingLevel: "low"`
- [ ] `--variant high` → `thinkingLevel: "high"`

### 3.3 Gemini 3 Flash Models
- [ ] `--variant minimal` → `thinkingLevel: "minimal"`
- [ ] `--variant low` → `thinkingLevel: "low"`
- [ ] `--variant medium` → `thinkingLevel: "medium"`
- [ ] `--variant high` → `thinkingLevel: "high"`

### 3.4 Gemini 2.5 Flash Thinking
- [ ] `--variant low` → `thinkingBudget: 8192`
- [ ] `--variant medium` → `thinkingBudget: 12288`
- [ ] `--variant high` → `thinkingBudget: 16384`
- [ ] `--variant max` → `thinkingBudget: 24576`

---

## 4. Plugin Compatibility Verification

### 4.1 Plugin Model Unaffected
```bash
# If opencode-antigravity-auth plugin installed
opencode run "test" --model google/antigravity-claude-sonnet-4-6-thinking --variant max
```
- [ ] Plugin provider works independently
- [ ] Manager sync does not interfere with plugin accounts
- [ ] Both can coexist

---

## 5. Clear/Restore Verification

### 5.1 Clear Config
- [ ] Removes `provider.antigravity-manager`
- [ ] Optional: clears legacy entries from `provider.google` and `provider.anthropic`
- [ ] Preserves other providers

### 5.2 Restore Function
| Backup Type | Expected Result |
|-------------|-----------------|
| New suffix (`.antigravity-manager.bak`) | Restores successfully |
| Old suffix (`.antigravity.bak`) | Restores successfully (backward compatible) |
| Both exist | Prefers new suffix |
| None exists | Shows "No backup files found" error |

---

## 6. Pass/Fail Summary Table

| Test Category | Test Item | Status |
|---------------|-----------|--------|
| Pre-check | Backup path correct | ⬜ Pass / ⬜ Fail |
| Pre-check | Plugin detection works | ⬜ Pass / ⬜ Fail |
| Sync | Provider created correctly | ⬜ Pass / ⬜ Fail |
| Sync | Existing providers preserved | ⬜ Pass / ⬜ Fail |
| Sync | Accounts v3 structure valid | ⬜ Pass / ⬜ Fail |
| Variants | Claude thinking budgets | ⬜ Pass / ⬜ Fail |
| Variants | Gemini 3 Pro levels | ⬜ Pass / ⬜ Fail |
| Variants | Gemini 3 Flash levels | ⬜ Pass / ⬜ Fail |
| Variants | Gemini 2.5 thinking budgets | ⬜ Pass / ⬜ Fail |
| Compatibility | Plugin unaffected | ⬜ Pass / ⬜ Fail |
| Clear/Restore | Clear removes manager provider | ⬜ Pass / ⬜ Fail |
| Clear/Restore | Restore with new suffix | ⬜ Pass / ⬜ Fail |
| Clear/Restore | Restore with old suffix | ⬜ Pass / ⬜ Fail |

---

## 7. Troubleshooting Notes

### Issue: Sync fails with "Failed to get OpenCode config directory"
**Cause:** Cannot determine home directory  
**Fix:** Ensure `HOME` (Unix) or `USERPROFILE` (Windows) env var is set

### Issue: Variant not applied
**Cause:** Model ID mismatch or variant type not defined  
**Fix:** Check model ID in catalog matches request; verify `variant_type` in `build_model_catalog()`

### Issue: Backup not created
**Cause:** Backup file already exists (idempotent)  
**Fix:** Delete existing `.bak` files manually if you need fresh backup

### Issue: Accounts not exported
**Cause:** All accounts disabled or `sync_accounts` not checked  
**Fix:** Enable at least one account; check "Sync accounts" option in UI

### Issue: Plugin conflicts with manager provider
**Cause:** Both using same model IDs  
**Fix:** Use different model IDs or disable one provider

### Issue: Restore fails
**Cause:** Backup files missing or permissions  
**Check:** 
```bash
ls -la ~/.config/opencode/*.bak
```

---

## Test Environment

- **OS**: 
- **OpenCode Version**: 
- **Antigravity Manager Version**: 
- **Test Date**: 
- **Tester**: 
