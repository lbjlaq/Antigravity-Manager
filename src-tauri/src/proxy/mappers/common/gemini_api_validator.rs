//! Gemini API format validator
//!
//! Validates that Gemini 3.x uses thinkingLevel API and Gemini 2.5 uses thinkingBudget API
//! Catches API format mismatches before sending to upstream

use serde_json::Value;
use crate::proxy::mappers::common::gemini_detection::is_gemini_3_model;

#[derive(Debug, Clone, PartialEq)]
pub enum GeminiApiValidationError {
    /// Gemini 3.x model using thinkingBudget (should use thinkingLevel)
    Gemini3WithBudget { model: String },

    /// Gemini 2.5 model using thinkingLevel (should use thinkingBudget)
    Gemini25WithLevel { model: String },

    /// thinkingConfig missing for thinking-enabled model
    MissingThinkingConfig { model: String },

    /// Invalid thinkingLevel value
    InvalidThinkingLevel { model: String, level: String },
}

impl std::fmt::Display for GeminiApiValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gemini3WithBudget { model } => {
                write!(f, "Gemini 3.x model '{}' must use thinkingLevel API, not thinkingBudget", model)
            }
            Self::Gemini25WithLevel { model } => {
                write!(f, "Gemini 2.5 model '{}' must use thinkingBudget API, not thinkingLevel", model)
            }
            Self::MissingThinkingConfig { model } => {
                write!(f, "Thinking-enabled model '{}' missing thinkingConfig", model)
            }
            Self::InvalidThinkingLevel { model, level } => {
                write!(f, "Model '{}' has invalid thinkingLevel: '{}' (must be MINIMAL/LOW/MEDIUM/HIGH)", model, level)
            }
        }
    }
}

impl std::error::Error for GeminiApiValidationError {}

/// Validates Gemini API request format
///
/// # Arguments
/// * `model` - The Gemini model name
/// * `request_body` - The upstream request body JSON
///
/// # Returns
/// * `Ok(())` if validation passes
/// * `Err(GeminiApiValidationError)` if validation fails
pub fn validate_gemini_request(
    model: &str,
    request_body: &Value,
) -> Result<(), GeminiApiValidationError> {
    // Only validate Gemini models
    if !model.starts_with("gemini-") {
        return Ok(());
    }

    let thinking_config = request_body
        .get("generationConfig")
        .and_then(|gc| gc.get("thinkingConfig"));

    // If no thinkingConfig, validation passes (non-thinking request)
    let Some(thinking_config) = thinking_config else {
        return Ok(());
    };

    let has_budget = thinking_config.get("thinkingBudget").is_some();
    let has_level = thinking_config.get("thinkingLevel").is_some();

    if is_gemini_3_model(model) {
        // Gemini 3.x: MUST use thinkingLevel, NOT thinkingBudget
        if has_budget && !has_level {
            return Err(GeminiApiValidationError::Gemini3WithBudget {
                model: model.to_string(),
            });
        }

        // Validate thinkingLevel value
        if let Some(level) = thinking_config.get("thinkingLevel").and_then(|v| v.as_str()) {
            let valid_levels = if model.contains("-flash") {
                // Flash: 4 levels
                vec!["MINIMAL", "LOW", "MEDIUM", "HIGH"]
            } else {
                // Pro: 2 levels
                vec!["LOW", "HIGH"]
            };

            if !valid_levels.contains(&level) {
                return Err(GeminiApiValidationError::InvalidThinkingLevel {
                    model: model.to_string(),
                    level: level.to_string(),
                });
            }
        }
    } else {
        // Gemini 2.5: MUST use thinkingBudget, NOT thinkingLevel
        if has_level && !has_budget {
            return Err(GeminiApiValidationError::Gemini25WithLevel {
                model: model.to_string(),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_gemini_3_with_thinking_level_passes() {
        let request = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingLevel": "HIGH"
                }
            }
        });

        assert!(validate_gemini_request("gemini-3-pro-high", &request).is_ok());
    }

    #[test]
    fn test_gemini_3_with_thinking_budget_fails() {
        let request = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 16000
                }
            }
        });

        let result = validate_gemini_request("gemini-3-pro-high", &request);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), GeminiApiValidationError::Gemini3WithBudget { .. }));
    }

    #[test]
    fn test_gemini_25_with_thinking_budget_passes() {
        let request = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 16000
                }
            }
        });

        assert!(validate_gemini_request("gemini-2.5-flash-thinking", &request).is_ok());
    }

    #[test]
    fn test_gemini_25_with_thinking_level_fails() {
        let request = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingLevel": "HIGH"
                }
            }
        });

        let result = validate_gemini_request("gemini-2.5-pro-thinking", &request);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), GeminiApiValidationError::Gemini25WithLevel { .. }));
    }

    #[test]
    fn test_flash_invalid_level_fails() {
        let request = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingLevel": "ULTRA"  // Invalid level
                }
            }
        });

        let result = validate_gemini_request("gemini-3-flash", &request);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), GeminiApiValidationError::InvalidThinkingLevel { .. }));
    }

    #[test]
    fn test_pro_medium_level_fails() {
        let request = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingLevel": "MEDIUM"  // Pro doesn't support MEDIUM
                }
            }
        });

        let result = validate_gemini_request("gemini-3-pro-high", &request);
        assert!(result.is_err());
    }

    #[test]
    fn test_non_thinking_request_passes() {
        let request = json!({
            "generationConfig": {
                "temperature": 0.7
            }
        });

        assert!(validate_gemini_request("gemini-3-flash", &request).is_ok());
    }
}
