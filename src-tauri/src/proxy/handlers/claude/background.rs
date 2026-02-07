// Background Task Detection
// Detects and routes background tasks to cheaper models

use crate::proxy::mappers::claude::ClaudeRequest;

// Model constant for background tasks
pub const INTERNAL_BACKGROUND_TASK: &str = "internal-background-task";

/// Background task type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackgroundTaskType {
    TitleGeneration,      // Title generation
    SimpleSummary,        // Simple summary
    ContextCompression,   // Context compression
    PromptSuggestion,     // Prompt suggestion
    SystemMessage,        // System message
    EnvironmentProbe,     // Environment probe
}

/// Title generation keywords
const TITLE_KEYWORDS: &[&str] = &[
    "write a 5-10 word title",
    "Please write a 5-10 word title",
    "Respond with the title",
    "Generate a title for",
    "Create a brief title",
    "title for the conversation",
    "conversation title",
    "生成标题",
    "为对话起个标题",
];

/// Summary generation keywords
const SUMMARY_KEYWORDS: &[&str] = &[
    "Summarize this coding conversation",
    "Summarize the conversation",
    "Concise summary",
    "in under 50 characters",
    "compress the context",
    "Provide a concise summary",
    "condense the previous messages",
    "shorten the conversation history",
    "extract key points from",
];

/// Suggestion generation keywords
const SUGGESTION_KEYWORDS: &[&str] = &[
    "prompt suggestion generator",
    "suggest next prompts",
    "what should I ask next",
    "generate follow-up questions",
    "recommend next steps",
    "possible next actions",
];

/// System message keywords
const SYSTEM_KEYWORDS: &[&str] = &[
    "Warmup",
    "<system-reminder>",
    "This is a system message",
];

/// Environment probe keywords
const PROBE_KEYWORDS: &[&str] = &[
    "check current directory",
    "list available tools",
    "verify environment",
    "test connection",
];

/// Detect background task and return task type
pub fn detect_background_task_type(request: &ClaudeRequest) -> Option<BackgroundTaskType> {
    let last_user_msg = extract_last_user_message_for_detection(request)?;
    let preview = last_user_msg.chars().take(500).collect::<String>();
    
    // Length filter: background tasks usually don't exceed 800 characters
    if last_user_msg.len() > 800 {
        return None;
    }
    
    // Match by priority
    if matches_keywords(&preview, SYSTEM_KEYWORDS) {
        return Some(BackgroundTaskType::SystemMessage);
    }
    
    if matches_keywords(&preview, TITLE_KEYWORDS) {
        return Some(BackgroundTaskType::TitleGeneration);
    }
    
    if matches_keywords(&preview, SUMMARY_KEYWORDS) {
        if preview.contains("in under 50 characters") {
            return Some(BackgroundTaskType::SimpleSummary);
        }
        return Some(BackgroundTaskType::ContextCompression);
    }
    
    if matches_keywords(&preview, SUGGESTION_KEYWORDS) {
        return Some(BackgroundTaskType::PromptSuggestion);
    }
    
    if matches_keywords(&preview, PROBE_KEYWORDS) {
        return Some(BackgroundTaskType::EnvironmentProbe);
    }
    
    None
}

/// Helper function: keyword matching
fn matches_keywords(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| text.contains(kw))
}

/// Helper function: extract last user message (for detection)
fn extract_last_user_message_for_detection(request: &ClaudeRequest) -> Option<String> {
    request.messages.iter().rev()
        .filter(|m| m.role == "user")
        .find_map(|m| {
            let content = match &m.content {
                crate::proxy::mappers::claude::models::MessageContent::String(s) => s.to_string(),
                crate::proxy::mappers::claude::models::MessageContent::Array(arr) => {
                    arr.iter()
                        .filter_map(|block| match block {
                            crate::proxy::mappers::claude::models::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                }
            };
            
            if content.trim().is_empty() 
                || content.starts_with("Warmup") 
                || content.contains("<system-reminder>") 
            {
                None 
            } else {
                Some(content)
            }
        })
}

/// Select appropriate model based on background task type
pub fn select_background_model(task_type: BackgroundTaskType) -> &'static str {
    match task_type {
        BackgroundTaskType::TitleGeneration => INTERNAL_BACKGROUND_TASK,
        BackgroundTaskType::SimpleSummary => INTERNAL_BACKGROUND_TASK,
        BackgroundTaskType::SystemMessage => INTERNAL_BACKGROUND_TASK,
        BackgroundTaskType::PromptSuggestion => INTERNAL_BACKGROUND_TASK,
        BackgroundTaskType::EnvironmentProbe => INTERNAL_BACKGROUND_TASK,
        BackgroundTaskType::ContextCompression => INTERNAL_BACKGROUND_TASK,
    }
}
