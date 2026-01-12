// Claude 数据模型
// Claude 协议相关数据模型

use serde::{Deserialize, Serialize};

/// Claude API 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
    /// Output configuration for effort level (Claude API v2.0.67+)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<OutputConfig>,

    // 🆕 Story #9: Tool calling mode configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
}

/// Thinking 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingConfig {
    #[serde(rename = "type")]
    pub type_: String, // "enabled"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u32>,
}

/// System Prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SystemPrompt {
    String(String),
    Array(Vec<SystemBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
}

/// Message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: MessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    String(String),
    Array(Vec<ContentBlock>),
}

/// Content Block (Claude)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<serde_json::Value>,
    },

    #[serde(rename = "image")]
    Image {
        source: ImageSource,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<serde_json::Value>,
    },

    #[serde(rename = "document")]
    Document {
        source: DocumentSource,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<serde_json::Value>,
    },

    #[serde(rename = "redacted_thinking")]
    RedactedThinking { data: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<serde_json::Value>,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value, // Changed from String to Value to support Array of Blocks
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },

    #[serde(rename = "server_tool_use")]
    ServerToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    #[serde(rename = "web_search_tool_result")]
    WebSearchToolResult {
        tool_use_id: String,
        content: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSource {
    #[serde(rename = "type")]
    pub source_type: String, // "base64"
    pub media_type: String, // e.g. "application/pdf"
    pub data: String,       // base64 data
}

/// Tool - supports both client tools (with input_schema) and server tools (like web_search)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Tool type - for server tools like "web_search_20250305"
    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// Tool name - "web_search" for server tools, custom name for client tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Input schema - required for client tools, absent for server tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
}

impl Tool {
    /// Check if this is the web_search server tool
    pub fn is_web_search(&self) -> bool {
        // Check by type (preferred for server tools)
        if let Some(ref t) = self.type_ {
            if t.starts_with("web_search") {
                return true;
            }
        }
        // Check by name (fallback)
        if let Some(ref n) = self.name {
            if n == "web_search" {
                return true;
            }
        }
        false
    }

    /// Get the effective tool name
    #[allow(dead_code)]
    pub fn get_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            // For server tools, derive name from type
            if let Some(ref t) = self.type_ {
                if t.starts_with("web_search") {
                    return "web_search".to_string();
                }
            }
            "unknown".to_string()
        })
    }
}

/// Metadata
/// Extended in Story #4 to support Cloud AI Companion integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    /// User ID for session tracking (Story #3)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Workspace ID for Cloud AI Companion (Story #4 - Optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,

    /// Cloud AI Companion Project ID (Story #4 - Optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloudaicompanion_project: Option<String>,
}

/// Output Configuration (Claude API v2.0.67+)
/// Controls effort level for model reasoning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Effort level: "high", "medium", "low"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

// ========== Story #9: Tool Configuration Modes ==========

/// Tool calling configuration (Claude API)
/// Defines how the model should use tools in requests
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
#[derive(Default)]
pub enum ToolChoice {
    /// Let the model decide when to use tools (maps to AUTO mode)
    #[default]
    Auto,

    /// Force the model to use at least one tool (maps to ANY mode)
    Any,

    /// Disable all tool calling for this request (maps to NONE mode)
    #[serde(rename = "none")]
    None,

    /// Force usage of a specific tool (maps to VALIDATED mode with allowedFunctionNames)
    Tool { name: String },
}


impl ToolChoice {
    /// Convert to Gemini Protocol mode string
    pub fn to_gemini_mode(&self) -> &'static str {
        match self {
            ToolChoice::Auto => "AUTO",
            ToolChoice::Any => "ANY",
            ToolChoice::None => "NONE",
            ToolChoice::Tool { .. } => "VALIDATED",
        }
    }

    /// Get specific tool name if Tool variant
    pub fn get_tool_name(&self) -> Option<&str> {
        match self {
            ToolChoice::Tool { name } => Some(name.as_str()),
            _ => None,
        }
    }
}

/// Claude API 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub role: String,
    pub model: String,
    pub content: Vec<ContentBlock>,
    pub stop_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    pub usage: Usage,
}

/// Usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_tool_use: Option<serde_json::Value>,
}

// ========== Gemini 数据模型 ==========

/// Gemini Content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiContent {
    pub role: String,
    pub parts: Vec<GeminiPart>,
}

/// Gemini Part
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "thoughtSignature")]
    pub thought_signature: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "functionCall")]
    pub function_call: Option<FunctionCall>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "functionResponse")]
    pub function_response: Option<FunctionResponse>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "inlineData")]
    pub inline_data: Option<InlineData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResponse {
    pub name: String,
    pub response: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineData {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub data: String,
}

/// Gemini 完整响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<Candidate>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "usageMetadata")]
    pub usage_metadata: Option<UsageMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "modelVersion")]
    pub model_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "responseId")]
    pub response_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "finishReason")]
    pub finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "groundingMetadata")]
    pub grounding_metadata: Option<GroundingMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "promptTokenCount")]
    pub prompt_token_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "candidatesTokenCount")]
    pub candidates_token_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "totalTokenCount")]
    pub total_token_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "cachedContentTokenCount")]
    pub cached_content_token_count: Option<u32>,
}

// ========== Grounding Metadata (for googleSearch results) ==========

/// Gemini Grounding Metadata - contains search results from googleSearch tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingMetadata {
    #[serde(rename = "webSearchQueries")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_search_queries: Option<Vec<String>>,

    #[serde(rename = "groundingChunks")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_chunks: Option<Vec<GroundingChunk>>,

    #[serde(rename = "groundingSupports")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_supports: Option<Vec<GroundingSupport>>,

    #[serde(rename = "searchEntryPoint")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_entry_point: Option<SearchEntryPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingChunk {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web: Option<WebSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingSupport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment: Option<TextSegment>,
    #[serde(rename = "groundingChunkIndices")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_chunk_indices: Option<Vec<i32>>,
    #[serde(rename = "confidenceScores")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence_scores: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextSegment {
    #[serde(rename = "startIndex")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_index: Option<i32>,
    #[serde(rename = "endIndex")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_index: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchEntryPoint {
    #[serde(rename = "renderedContent")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered_content: Option<String>,
}

// ========== Story #8: Violation Tracking ==========

/// 🆕 Story #8 Step 7: ViolationInfo structure
/// Передаёт информацию о thinking violations из mapper в handler
/// для последующей записи в ProxyMonitor metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ViolationInfo {
    /// Budget constraint violation detected (maxTokens ≤ thinkingBudget)
    /// Story #6 integration point
    pub budget_violation: bool,

    /// Position enforcement violation detected (thinking not first)
    /// Story #7 integration point
    pub position_violation: bool,

    /// Index where position violation occurred (for histogram)
    pub position_index: Option<usize>,

    /// Role of message with position violation ("user" or "model")
    pub position_role: Option<String>,
}

impl ViolationInfo {
    /// Create new empty ViolationInfo (no violations)
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if any violation occurred
    pub fn has_violations(&self) -> bool {
        self.budget_violation || self.position_violation
    }

    /// Record budget violation
    pub fn record_budget_violation(&mut self) {
        self.budget_violation = true;
    }

    /// Record position violation with context
    pub fn record_position_violation(&mut self, index: usize, role: &str) {
        self.position_violation = true;
        self.position_index = Some(index);
        self.position_role = Some(role.to_string());
    }
}
