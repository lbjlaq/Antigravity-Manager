// Debug variants for Claude model requests
// Try different combinations to find what Google expects

use std::collections::HashMap;

/// Different User-Agent variants to test
pub fn get_user_agent_variants() -> Vec<&'static str> {
    vec![
        // Current (baseline)
        "antigravity/1.11.9 windows/amd64",

        // Newer versions
        "antigravity/1.12.0 windows/amd64",
        "antigravity/1.13.0 windows/amd64",
        "antigravity/2.0.0 windows/amd64",

        // Different OS
        "antigravity/1.11.9 darwin/arm64",
        "antigravity/1.11.9 darwin/amd64",
        "antigravity/1.11.9 linux/amd64",

        // Different format
        "Antigravity/1.11.9",
        "antigravity-ide/1.11.9",
        "Google-Antigravity/1.11.9",

        // With additional info
        "antigravity/1.11.9 (claude-models)",
        "antigravity/1.11.9 partner-models",
    ]
}

/// Different requestType variants to test for Claude models
pub fn get_request_type_variants() -> Vec<&'static str> {
    vec![
        "agent",              // Current default
        "partner_model",      // Hypothesis A
        "claude",             // Hypothesis B
        "anthropic",          // Hypothesis C
        "partner",            // Hypothesis D
        "claude_agent",       // Hypothesis E
        "anthropic_model",    // Hypothesis F
        "third_party_model",  // Hypothesis G
        "vertex_claude",      // Hypothesis H
    ]
}

/// Additional headers that might be required for Claude models
pub fn get_additional_headers_variants() -> Vec<HashMap<&'static str, &'static str>> {
    vec![
        // Variant 1: No additional headers (baseline)
        HashMap::new(),

        // Variant 2: Google API client header
        {
            let mut h = HashMap::new();
            h.insert("X-Goog-Api-Client", "antigravity/1.11.9");
            h
        },

        // Variant 3: Client version
        {
            let mut h = HashMap::new();
            h.insert("X-Client-Version", "1.11.9");
            h
        },

        // Variant 4: Partner model flag
        {
            let mut h = HashMap::new();
            h.insert("X-Partner-Model", "true");
            h
        },

        // Variant 5: Anthropic specific
        {
            let mut h = HashMap::new();
            h.insert("X-Model-Provider", "anthropic");
            h
        },

        // Variant 6: Combination
        {
            let mut h = HashMap::new();
            h.insert("X-Goog-Api-Client", "antigravity/1.11.9");
            h.insert("X-Partner-Model", "true");
            h.insert("X-Model-Provider", "anthropic");
            h
        },
    ]
}

/// Different requestId prefixes to test
pub fn get_request_id_prefixes() -> Vec<&'static str> {
    vec![
        "agent-",           // Current
        "claude-",          // For Claude models
        "partner-",         // For partner models
        "anthropic-",       // For Anthropic
        "vertex-",          // For Vertex AI
        "request-",         // Generic
        "",                 // No prefix
    ]
}

/// Test configuration combining different variants
#[derive(Debug, Clone)]
pub struct TestVariant {
    pub name: String,
    pub user_agent: String,
    pub request_type: String,
    pub additional_headers: HashMap<String, String>,
    pub request_id_prefix: String,
}

impl TestVariant {
    pub fn generate_test_variants() -> Vec<Self> {
        let mut variants = Vec::new();

        // Priority variants to test first
        variants.push(TestVariant {
            name: "Baseline (current)".to_string(),
            user_agent: "antigravity/1.11.9 windows/amd64".to_string(),
            request_type: "agent".to_string(),
            additional_headers: HashMap::new(),
            request_id_prefix: "agent-".to_string(),
        });

        variants.push(TestVariant {
            name: "Partner Model Type".to_string(),
            user_agent: "antigravity/1.11.9 windows/amd64".to_string(),
            request_type: "partner_model".to_string(),
            additional_headers: HashMap::new(),
            request_id_prefix: "agent-".to_string(),
        });

        variants.push(TestVariant {
            name: "Anthropic Request Type".to_string(),
            user_agent: "antigravity/1.11.9 windows/amd64".to_string(),
            request_type: "anthropic".to_string(),
            additional_headers: HashMap::new(),
            request_id_prefix: "agent-".to_string(),
        });

        variants.push(TestVariant {
            name: "With Partner Headers".to_string(),
            user_agent: "antigravity/1.11.9 windows/amd64".to_string(),
            request_type: "agent".to_string(),
            additional_headers: {
                let mut h = HashMap::new();
                h.insert("X-Partner-Model".to_string(), "true".to_string());
                h.insert("X-Model-Provider".to_string(), "anthropic".to_string());
                h
            },
            request_id_prefix: "agent-".to_string(),
        });

        variants.push(TestVariant {
            name: "Newer User-Agent".to_string(),
            user_agent: "antigravity/1.13.0 windows/amd64".to_string(),
            request_type: "agent".to_string(),
            additional_headers: HashMap::new(),
            request_id_prefix: "agent-".to_string(),
        });

        variants.push(TestVariant {
            name: "Claude Specific".to_string(),
            user_agent: "antigravity/1.11.9 windows/amd64".to_string(),
            request_type: "claude".to_string(),
            additional_headers: {
                let mut h = HashMap::new();
                h.insert("X-Model-Provider".to_string(), "anthropic".to_string());
                h
            },
            request_id_prefix: "claude-".to_string(),
        });

        variants
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_variants() {
        let variants = TestVariant::generate_test_variants();
        assert!(!variants.is_empty());
        println!("Generated {} test variants", variants.len());
        for v in &variants {
            println!("  - {}", v.name);
        }
    }
}
