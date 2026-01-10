# Story QUOTA-001-01: Quota API Integration

**Story ID**: QUOTA-001-01
**Epic**: [QUOTA-001 - Proactive Quota Monitoring](../epics/proactive-quota-monitoring-epic.md)
**Priority**: P0 (Critical)
**Estimate**: 5 story points
**Status**: To Do
**Assignee**: TBD

---

## User Story

**As a** system administrator
**I want** the proxy to fetch real-time quota information from Google's API
**So that** we can make informed decisions about account selection and prevent 429 errors

---

## Context

Currently, the system only learns about quota exhaustion **after** receiving a 429 error. This is reactive and inefficient. Google Antigravity provides a `fetchAvailableModels` API that returns `remainingFraction` (0.0-1.0) for each model, allowing proactive quota management.

**Current Flow** (Reactive):
```
Request → API Call → 429 Error → Mark Account Limited → Switch Account
```

**Desired Flow** (Proactive):
```
Request → Check Quota Cache → If Low → Switch Account → API Call → Success
```

---

## Technical Details

### API Specification

**Reference**: [quota-apis.md#fetch-available-models](../antigravity/api/quota-apis.md#1-fetch-available-models-with-quota-info)

**Endpoint**:
```http
POST https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels
Authorization: Bearer {access_token}
Content-Type: application/json
```

**Request**:
```json
{
  "project": "bamboo-precept-lgxtn"
}
```

**Response**:
```json
{
  "models": {
    "gemini-2.5-flash": {
      "displayName": "Gemini 2.5 Flash",
      "supportedGenerationMethods": ["generateContent", "streamGenerateContent"],
      "quotaInfo": {
        "remainingFraction": 0.87,
        "resetTime": "2026-01-11T00:00:00Z"
      }
    },
    "claude-opus-4-5-thinking": {
      "displayName": "Claude Opus 4.5 with Thinking",
      "quotaInfo": {
        "remainingFraction": 0.05,
        "resetTime": "2026-01-11T00:00:00Z"
      }
    }
  }
}
```

**Key Fields**:
- `remainingFraction`: Float 0.0-1.0 representing remaining quota percentage
  - `1.0` = 100% quota remaining (unused)
  - `0.5` = 50% quota used
  - `0.0` = Quota fully exhausted
- `resetTime`: ISO 8601 timestamp of next quota reset (UTC midnight)

---

## Acceptance Criteria

### AC-1: API Client Implementation

**Given** a valid OAuth access token and project ID
**When** calling `fetch_available_models(access_token, project_id)`
**Then** the system should:
- [ ] Send POST request to correct endpoint
- [ ] Include proper authentication headers
- [ ] Parse JSON response successfully
- [ ] Return HashMap<String, QuotaInfo> with all models

**Test Cases**:
```rust
#[tokio::test]
async fn test_fetch_available_models_success() {
    let fetcher = QuotaFetcher::new();
    let result = fetcher.fetch_available_models(
        "ya29.mock_token",
        "test-project-123"
    ).await;

    assert!(result.is_ok());
    let quotas = result.unwrap();
    assert!(quotas.contains_key("gemini-2.5-flash"));
    assert!(quotas["gemini-2.5-flash"].remaining_fraction >= 0.0);
    assert!(quotas["gemini-2.5-flash"].remaining_fraction <= 1.0);
}
```

---

### AC-2: Data Structure Parsing

**Given** a valid API response
**When** parsing quota information
**Then** the system should:
- [ ] Extract `remainingFraction` as f64 (0.0-1.0)
- [ ] Parse `resetTime` as ISO 8601 timestamp
- [ ] Convert resetTime to SystemTime
- [ ] Handle missing quotaInfo gracefully

**Data Structures**:
```rust
#[derive(Debug, Clone, Deserialize)]
pub struct QuotaInfo {
    #[serde(rename = "remainingFraction")]
    pub remaining_fraction: f64,

    #[serde(rename = "resetTime")]
    pub reset_time: String,  // ISO 8601
}

impl QuotaInfo {
    pub fn to_system_time(&self) -> Result<SystemTime, String> {
        let dt = chrono::DateTime::parse_from_rfc3339(&self.reset_time)
            .map_err(|e| format!("Invalid resetTime: {}", e))?;

        Ok(SystemTime::UNIX_EPOCH + Duration::from_secs(dt.timestamp() as u64))
    }
}
```

---

### AC-3: Error Handling

**Given** various API error scenarios
**When** fetching quota information
**Then** the system should:
- [ ] Handle 401 (token refresh and retry)
- [ ] Handle 403 (mark account as forbidden)
- [ ] Handle 404 (endpoint fallback to daily)
- [ ] Handle 429 (exponential backoff)
- [ ] Handle 500/503 (retry with backoff)
- [ ] Return descriptive error messages

**Error Handling Pattern** ([error-pattern-catalog.md](../antigravity/reference/error-pattern-catalog.md)):
```rust
pub async fn fetch_available_models(
    &self,
    access_token: &str,
    project_id: &str
) -> Result<HashMap<String, QuotaInfo>, String> {
    let url = "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels";

    let response = self.client.post(url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({"project": project_id}))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status().as_u16();

    match status {
        200 => {
            let data: ModelsResponse = response.json().await
                .map_err(|e| format!("Parse error: {}", e))?;
            Ok(parse_quota_info(data))
        }
        401 => Err("Authentication failed - token expired".to_string()),
        403 => Err("Permission denied - account may be blocked".to_string()),
        404 => {
            // Fallback to daily endpoint
            self.fetch_from_daily_endpoint(access_token, project_id).await
        }
        429 => Err("Rate limited - retry after delay".to_string()),
        _ => Err(format!("API error: {}", status))
    }
}
```

---

### AC-4: Integration with TokenManager

**Given** TokenManager needs quota information
**When** selecting accounts
**Then** the system should:
- [ ] Expose `QuotaManager` to `TokenManager`
- [ ] Allow querying quota for specific (account, model) pair
- [ ] Return cached quota if fresh (<5 min)
- [ ] Fetch from API if cache miss or stale

---

## Implementation Tasks

### Task 1: Create QuotaManager Module

**File**: `src-tauri/src/proxy/quota_manager.rs`

**Components**:
```rust
pub struct QuotaManager {
    /// Quota cache: "account:model" -> CachedQuotaInfo
    cache: Arc<DashMap<String, CachedQuotaInfo>>,

    /// API client for fetching quotas
    fetcher: QuotaFetcher,
}

pub struct QuotaFetcher {
    client: reqwest::Client,
    endpoint: String,
}

#[derive(Debug, Clone)]
pub struct CachedQuotaInfo {
    pub remaining_fraction: f64,
    pub reset_time: SystemTime,
    pub fetched_at: SystemTime,
    pub ttl: Duration,
}
```

**Estimated Effort**: 2 hours

---

### Task 2: Implement API Client

**Functions**:
```rust
impl QuotaFetcher {
    pub fn new() -> Self;

    pub async fn fetch_available_models(
        &self,
        access_token: &str,
        project_id: &str
    ) -> Result<HashMap<String, QuotaInfo>, String>;

    async fn fetch_from_daily_endpoint(
        &self,
        access_token: &str,
        project_id: &str
    ) -> Result<HashMap<String, QuotaInfo>, String>;
}
```

**Estimated Effort**: 3 hours

---

### Task 3: Response Parsing

**Functions**:
```rust
fn parse_quota_info(response: ModelsResponse) -> HashMap<String, QuotaInfo> {
    response.models
        .into_iter()
        .filter_map(|(model_name, model_info)| {
            model_info.quota_info.map(|qi| (model_name, qi))
        })
        .collect()
}
```

**Estimated Effort**: 1 hour

---

### Task 4: Unit Tests

**Test Coverage**:
- [ ] Successful API response parsing
- [ ] Error handling (401, 403, 404, 429, 500)
- [ ] Endpoint fallback logic
- [ ] QuotaInfo to SystemTime conversion
- [ ] Edge cases (empty response, missing quotaInfo)

**Estimated Effort**: 2 hours

---

### Task 5: Integration Tests

**Test Scenarios**:
- [ ] End-to-end quota fetch with real API (dev environment)
- [ ] Cache integration with TokenManager
- [ ] Error recovery (token refresh on 401)

**Estimated Effort**: 2 hours

---

## Dependencies

### Blocking Dependencies

**Required Before Start**:
- [ ] OAuth authentication working
- [ ] TokenManager module accessible
- [ ] reqwest HTTP client available

### External Dependencies

**Google APIs**:
- `fetchAvailableModels` endpoint availability
- OAuth token with correct scopes

---

## Testing Strategy

### Unit Tests

**Mock API Responses**:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{mock, server_url};

    #[tokio::test]
    async fn test_parse_remaining_fraction() {
        let mock_response = json!({
            "models": {
                "gemini-2.5-flash": {
                    "quotaInfo": {
                        "remainingFraction": 0.87,
                        "resetTime": "2026-01-11T00:00:00Z"
                    }
                }
            }
        });

        let quota = parse_quota_info(mock_response);
        assert_eq!(quota["gemini-2.5-flash"].remaining_fraction, 0.87);
    }

    #[tokio::test]
    async fn test_handle_401_error() {
        let _m = mock("POST", "/v1internal:fetchAvailableModels")
            .with_status(401)
            .create();

        let fetcher = QuotaFetcher::new_with_endpoint(&server_url());
        let result = fetcher.fetch_available_models("invalid_token", "project").await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Authentication failed"));
    }
}
```

### Integration Tests

**Test Environment**: Development proxy instance

**Scenarios**:
- Successful quota fetch for 11 accounts
- Cache population after fetch
- TokenManager reading from quota cache

---

## Reference Documentation

**Google Antigravity Reverse Engineering**:
- [Quota Management APIs](../antigravity/api/quota-apis.md) - Complete API documentation
- [Error Pattern Catalog](../antigravity/reference/error-pattern-catalog.md) - Error handling patterns
- [Complete Examples](../antigravity/examples/complete-examples.md) - Request/response examples

**Related PRD**:
- [Proactive Quota Monitoring PRD](../product-requirements/proactive-quota-monitoring-prd.md)

**Related Stories**:
- [QUOTA-001-02: Pre-Request Validation](QUOTA-001-02-pre-request-validation.md) - Uses quota data from this story
- [QUOTA-001-05: Quota Cache](QUOTA-001-05-quota-cache.md) - Storage for fetched quota data

---

## Definition of Done

- [ ] Code implemented and reviewed
- [ ] Unit tests passing (>80% coverage)
- [ ] Integration tests passing
- [ ] API successfully fetches quotas from Google
- [ ] Error handling tested for all scenarios
- [ ] Code merged to main branch
- [ ] Documentation updated

---

**Story History**:
- 2026-01-10: Story created
