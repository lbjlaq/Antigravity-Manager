# Fix Quota Refresh and Account Rotation Issues

Users report that some applications show "quota exhausted" even when they actually have credit. This is due to proxy-level rate limit locks being stale and the handler failing prematurely.

## Proposed Changes

### [Proxy Module]

#### [MODIFY] [token_manager.rs](file:///c:/AAAAAAAAAAA_temp/desktop/Orignal_Repo/Antigravity-Manager/src-tauri/src/proxy/token_manager.rs)
- Ensure `load_accounts` has a way to clear associated rate limit records if needed, or simply ensure we can access the tracker to clear individual accounts. (Actually, `TokenManager` already has `clear_rate_limit` method).

### [Commands Module]

#### [MODIFY] [mod.rs](file:///c:/AAAAAAAAAAA_temp/desktop/Orignal_Repo/Antigravity-Manager/src-tauri/src/commands/mod.rs)
- In `fetch_account_quota` and `refresh_all_quotas`, after successfully updating the quota on disk, also clear the rate limit lock in the active proxy service's `TokenManager`.
- This requires accessing the `ProxyServiceState` and calling `clear_rate_limit`.

### [Handlers Module]

#### [MODIFY] [openai.rs](file:///c:/AAAAAAAAAAA_temp/desktop/Orignal_Repo/Antigravity-Manager/src-tauri/src/proxy/handlers/openai.rs)
- Modify the `QUOTA_EXHAUSTED` check to `continue` the loop instead of returning an error immediately. This allows the proxy to try other accounts in the pool.
- Only return the error if it's the last attempt or all pool accounts have been tried.

## Verification Plan

### Manual Verification
1. **Trigger a Lockout**: Manually induce a `QUOTA_EXHAUSTED` error (e.g., by using an account that is actually out of quota) and verify the account is locked in the proxy (check logs for exponential backoff).
2. **Refresh Quota**: In the UI, click "Refresh Quota" for that account.
3. **Verify Unlock**: Attempt a new request through the proxy. It should now proceed using this account (if it's the best choice) instead of showing "All accounts exhausted".
4. **Verify Rotation**: Use two accounts, one with 0 quota. Send a request. Verify that even if the first account hit hits 429 Quota Exhausted, the proxy continues and successfully uses the second account in the same request.
