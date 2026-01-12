//! Grounding configuration for Claude models
//!
//! Provides Google Search integration and recitation policies for enhanced
//! model responses with grounded information.
//!
//! # Overview
//!
//! This module implements grounding configuration for Claude 4.5 Sonnet models
//! when accessing Google's Gemini API via Vertex AI. It enables:
//!
//! - **Google Search Retrieval**: Dynamic web search integration for real-time information
//! - **Recitation Policy**: Controls how the model handles quoted or cited content
//! - **Dynamic Retrieval**: Adaptive search triggering based on confidence thresholds
//!
//! # Story Context
//!
//! **Story-017-02**: Claude 4.5 Sonnet Standard - Tool Modes & Grounding
//! - Implements AC-4: Grounding Configuration (HIGH)
//! - Supports Google Search integration for claude-4.5-sonnet (modelId 333)
//! - Enables cited document policy for compliance

use serde::{Deserialize, Serialize};

/// Grounding configuration for Claude models accessing Gemini API
///
/// Combines Google Search retrieval capabilities with recitation policies
/// to provide grounded, citation-aware responses.
///
/// # Example
///
/// ```rust
/// use antigravity_tools_lib::proxy::mappers::claude::grounding::GroundingConfig;
///
/// // Create default grounding config (enabled)
/// let config = GroundingConfig::default();
/// assert!(config.google_search_retrieval.enabled);
///
/// // Create disabled grounding
/// let disabled = GroundingConfig::disabled();
/// assert!(!disabled.google_search_retrieval.enabled);
///
/// // Custom threshold
/// let custom = GroundingConfig::new().with_threshold(0.5);
/// assert_eq!(custom.google_search_retrieval.dynamic_retrieval_config.dynamic_threshold, 0.5);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingConfig {
    /// Google Search retrieval configuration
    #[serde(rename = "googleSearchRetrieval")]
    pub google_search_retrieval: GoogleSearchRetrieval,

    /// Recitation policy for quoted content
    #[serde(rename = "recitationPolicy")]
    pub recitation_policy: RecitationPolicy,
}

/// Google Search retrieval configuration
///
/// Controls when and how the model performs web searches to ground responses
/// in real-time information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleSearchRetrieval {
    /// Whether Google Search retrieval is enabled
    pub enabled: bool,

    /// Dynamic retrieval configuration
    #[serde(rename = "dynamicRetrievalConfig")]
    pub dynamic_retrieval_config: DynamicRetrievalConfig,
}

/// Dynamic retrieval configuration
///
/// Controls automatic triggering of web searches based on the model's
/// confidence in its existing knowledge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicRetrievalConfig {
    /// Retrieval mode: "MODE_DYNAMIC" for automatic triggering
    pub mode: String,

    /// Confidence threshold (0.0-1.0) below which search is triggered
    ///
    /// - 0.3 (default): Balanced - searches when moderately uncertain
    /// - 0.5: Conservative - only searches when quite uncertain
    /// - 0.1: Aggressive - searches frequently for fresh information
    #[serde(rename = "dynamicThreshold")]
    pub dynamic_threshold: f64,
}

/// Recitation policy configuration
///
/// Controls how the model handles quoted or cited content from its training data
/// or search results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecitationPolicy {
    /// Whether recitation policy is enabled
    pub enabled: bool,

    /// Policy type
    ///
    /// - "CITED_DOCUMENTS_ONLY": Only allow citations from search results
    /// - "BLOCK": Block all recited content (strictest)
    /// - "ALLOW": Allow recited content (most permissive)
    pub policy: String,
}

impl Default for GroundingConfig {
    /// Create default grounding configuration (enabled)
    ///
    /// Default settings:
    /// - Google Search: Enabled
    /// - Dynamic threshold: 0.3 (balanced)
    /// - Recitation policy: CITED_DOCUMENTS_ONLY
    fn default() -> Self {
        Self {
            google_search_retrieval: GoogleSearchRetrieval {
                enabled: true,
                dynamic_retrieval_config: DynamicRetrievalConfig {
                    mode: "MODE_DYNAMIC".to_string(),
                    dynamic_threshold: 0.3,
                },
            },
            recitation_policy: RecitationPolicy {
                enabled: true,
                policy: "CITED_DOCUMENTS_ONLY".to_string(),
            },
        }
    }
}

impl GroundingConfig {
    /// Create new grounding config with default settings
    ///
    /// # Example
    ///
    /// ```rust
    /// # use antigravity_tools_lib::proxy::mappers::claude::grounding::GroundingConfig;
    /// let config = GroundingConfig::new();
    /// assert!(config.google_search_retrieval.enabled);
    /// assert_eq!(config.google_search_retrieval.dynamic_retrieval_config.dynamic_threshold, 0.3);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Create disabled grounding configuration
    ///
    /// Use this when you want to explicitly disable grounding features
    /// while keeping the configuration structure.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use antigravity_tools_lib::proxy::mappers::claude::grounding::GroundingConfig;
    /// let config = GroundingConfig::disabled();
    /// assert!(!config.google_search_retrieval.enabled);
    /// assert!(!config.recitation_policy.enabled);
    /// ```
    pub fn disabled() -> Self {
        Self {
            google_search_retrieval: GoogleSearchRetrieval {
                enabled: false,
                dynamic_retrieval_config: DynamicRetrievalConfig {
                    mode: "MODE_DYNAMIC".to_string(),
                    dynamic_threshold: 0.3,
                },
            },
            recitation_policy: RecitationPolicy {
                enabled: false,
                policy: "CITED_DOCUMENTS_ONLY".to_string(),
            },
        }
    }

    /// Enable grounding with custom threshold
    ///
    /// # Arguments
    ///
    /// * `threshold` - Confidence threshold (0.0-1.0) below which search triggers
    ///
    /// # Example
    ///
    /// ```rust
    /// # use antigravity_tools_lib::proxy::mappers::claude::grounding::GroundingConfig;
    /// let config = GroundingConfig::new().with_threshold(0.5);
    /// assert_eq!(config.google_search_retrieval.dynamic_retrieval_config.dynamic_threshold, 0.5);
    /// ```
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.google_search_retrieval
            .dynamic_retrieval_config
            .dynamic_threshold = threshold;
        self
    }

    /// Set recitation policy
    ///
    /// # Arguments
    ///
    /// * `policy` - Policy type: "CITED_DOCUMENTS_ONLY", "BLOCK", or "ALLOW"
    ///
    /// # Example
    ///
    /// ```rust
    /// # use antigravity_tools_lib::proxy::mappers::claude::grounding::GroundingConfig;
    /// let config = GroundingConfig::new().with_policy("BLOCK");
    /// assert_eq!(config.recitation_policy.policy, "BLOCK");
    /// ```
    pub fn with_policy(mut self, policy: &str) -> Self {
        self.recitation_policy.policy = policy.to_string();
        self
    }

    /// Check if grounding is fully enabled
    ///
    /// Returns true only if both Google Search and recitation policy are enabled.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use antigravity_tools_lib::proxy::mappers::claude::grounding::GroundingConfig;
    /// let enabled = GroundingConfig::new();
    /// assert!(enabled.is_enabled());
    ///
    /// let disabled = GroundingConfig::disabled();
    /// assert!(!disabled.is_enabled());
    /// ```
    pub fn is_enabled(&self) -> bool {
        self.google_search_retrieval.enabled && self.recitation_policy.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test default grounding configuration
    #[test]
    fn test_default_grounding_config() {
        let config = GroundingConfig::default();
        assert!(config.google_search_retrieval.enabled);
        assert_eq!(
            config.google_search_retrieval.dynamic_retrieval_config.mode,
            "MODE_DYNAMIC"
        );
        assert_eq!(
            config
                .google_search_retrieval
                .dynamic_retrieval_config
                .dynamic_threshold,
            0.3
        );
        assert!(config.recitation_policy.enabled);
        assert_eq!(config.recitation_policy.policy, "CITED_DOCUMENTS_ONLY");
    }

    /// Test disabled grounding configuration
    #[test]
    fn test_disabled_grounding_config() {
        let config = GroundingConfig::disabled();
        assert!(!config.google_search_retrieval.enabled);
        assert!(!config.recitation_policy.enabled);
        assert!(!config.is_enabled());
    }

    /// Test custom threshold configuration
    #[test]
    fn test_custom_threshold() {
        let config = GroundingConfig::new().with_threshold(0.5);
        assert_eq!(
            config
                .google_search_retrieval
                .dynamic_retrieval_config
                .dynamic_threshold,
            0.5
        );
    }

    /// Test custom policy configuration
    #[test]
    fn test_custom_policy() {
        let config = GroundingConfig::new().with_policy("BLOCK");
        assert_eq!(config.recitation_policy.policy, "BLOCK");
    }

    /// Test is_enabled method
    #[test]
    fn test_is_enabled() {
        let enabled = GroundingConfig::new();
        assert!(enabled.is_enabled());

        let disabled = GroundingConfig::disabled();
        assert!(!disabled.is_enabled());
    }

    /// Test JSON serialization
    #[test]
    fn test_serialization() {
        let config = GroundingConfig::new();
        let json = serde_json::to_value(&config).unwrap();

        assert!(json.get("googleSearchRetrieval").is_some());
        assert_eq!(json["googleSearchRetrieval"]["enabled"], true);
        assert_eq!(
            json["googleSearchRetrieval"]["dynamicRetrievalConfig"]["mode"],
            "MODE_DYNAMIC"
        );
        assert_eq!(
            json["googleSearchRetrieval"]["dynamicRetrievalConfig"]["dynamicThreshold"],
            0.3
        );

        assert!(json.get("recitationPolicy").is_some());
        assert_eq!(json["recitationPolicy"]["enabled"], true);
        assert_eq!(json["recitationPolicy"]["policy"], "CITED_DOCUMENTS_ONLY");
    }

    /// Test JSON deserialization
    #[test]
    fn test_deserialization() {
        let json = r#"{
            "googleSearchRetrieval": {
                "enabled": true,
                "dynamicRetrievalConfig": {
                    "mode": "MODE_DYNAMIC",
                    "dynamicThreshold": 0.3
                }
            },
            "recitationPolicy": {
                "enabled": true,
                "policy": "CITED_DOCUMENTS_ONLY"
            }
        }"#;

        let config: GroundingConfig = serde_json::from_str(json).unwrap();
        assert!(config.google_search_retrieval.enabled);
        assert_eq!(config.recitation_policy.policy, "CITED_DOCUMENTS_ONLY");
    }

    /// Test builder pattern chaining
    #[test]
    fn test_builder_pattern() {
        let config = GroundingConfig::new()
            .with_threshold(0.7)
            .with_policy("BLOCK");

        assert_eq!(
            config
                .google_search_retrieval
                .dynamic_retrieval_config
                .dynamic_threshold,
            0.7
        );
        assert_eq!(config.recitation_policy.policy, "BLOCK");
    }
}
