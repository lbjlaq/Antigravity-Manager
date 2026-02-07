// Layer 3 Context Compression
// Fork conversation + XML summary generation

use std::sync::Arc;
use serde_json::Value;
use tracing::{debug, info};

use crate::proxy::mappers::claude::{ClaudeRequest, models::{Message, MessageContent}};
use crate::proxy::mappers::context_manager::ContextManager;
use super::background::INTERNAL_BACKGROUND_TASK;

/// XML Summary Prompt Template
/// Borrowed from Practical-Guide-to-Context-Engineering + Claude Code official practice
pub const CONTEXT_SUMMARY_PROMPT: &str = r#"You are a context compression specialist. Your task is to create a structured XML snapshot of the conversation history.

This snapshot will become the Agent's ONLY memory of the past. All key details, plans, errors, and user instructions MUST be preserved.

First, think through the entire history in a private <scratchpad>. Review the user's overall goal, the agent's actions, tool outputs, file modifications, and any unresolved issues. Identify every piece of information critical for future actions.

After reasoning, generate the final <state_snapshot> XML object. Information must be extremely dense. Omit any irrelevant conversational filler.

The structure MUST be as follows:

<state_snapshot>
  <overall_goal>
    <!-- Describe the user's high-level goal in one concise sentence -->
  </overall_goal>
  
  <technical_context>
    <!-- Tech stack: frameworks, languages, toolchain, dependency versions -->
  </technical_context>
  
  <file_system_state>
    <!-- List files that were created, read, modified, or deleted. Note their status -->
  </file_system_state>
  
  <code_changes>
    <!-- Key code snippets (preserve function signatures and important logic) -->
  </code_changes>
  
  <debugging_history>
    <!-- List all errors encountered, with stack traces, and how they were fixed -->
  </debugging_history>
  
  <current_plan>
    <!-- Step-by-step plan. Mark completed steps -->
  </current_plan>
  
  <user_preferences>
    <!-- User's work preferences for this project (test commands, code style, etc.) -->
  </user_preferences>
  
  <key_decisions>
    <!-- Critical architectural decisions and design choices -->
  </key_decisions>
  
  <latest_thinking_signature>
    <!-- [CRITICAL] Preserve the last valid thinking signature -->
    <!-- Format: base64-encoded signature string -->
    <!-- This MUST be copied exactly as-is, no modifications -->
  </latest_thinking_signature>
</state_snapshot>

**IMPORTANT**:
1. Code snippets must be complete, including function signatures and key logic
2. Error messages must be preserved verbatim, including line numbers and stacks
3. File paths must use absolute paths
4. The thinking signature must be copied exactly, no modifications
"#;

/// Call Gemini API synchronously and return the response text
/// 
/// Used for internal operations that need to wait for a complete response,
/// such as generating summaries or other background tasks.
async fn call_gemini_sync(
    model: &str,
    request: &ClaudeRequest,
    token_manager: &Arc<crate::proxy::TokenManager>,
    trace_id: &str,
) -> Result<String, String> {
    let token_lease = token_manager
        .get_token("gemini", false, None, model)
        .await
        .map_err(|e| format!("Failed to get account: {}", e))?;

    let access_token = token_lease.access_token.clone();
    let project_id = token_lease.project_id.clone();
    
    let gemini_body = crate::proxy::mappers::claude::transform_claude_request_in(request, &project_id, false)
        .map_err(|e| format!("Failed to transform request: {}", e))?;
    
    let upstream_url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
        model
    );
    
    debug!("[{}] Calling Gemini API: {}", trace_id, model);
    
    let response = reqwest::Client::new()
        .post(&upstream_url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .json(&gemini_body)
        .send()
        .await
        .map_err(|e| format!("API call failed: {}", e))?;
    
    if !response.status().is_success() {
        return Err(format!(
            "API returned {}: {}", 
            response.status(), 
            response.text().await.unwrap_or_default()
        ));
    }
    
    let gemini_response: Value = response.json().await
        .map_err(|e| format!("Failed to parse response: {}", e))?;
    
    gemini_response
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Failed to extract text from response".to_string())
}

/// Try to compress context by generating an XML summary and forking the conversation
/// 
/// This function:
/// 1. Extracts the last valid thinking signature
/// 2. Calls a cheap model to generate XML summary
/// 3. Creates a new message sequence with summary as prefix
/// 4. Preserves the signature in the summary
/// 5. Returns the forked request
pub async fn try_compress_with_summary(
    original_request: &ClaudeRequest,
    trace_id: &str,
    token_manager: &Arc<crate::proxy::TokenManager>,
) -> Result<ClaudeRequest, String> {
    info!("[{}] [Layer-3] Starting context compression with XML summary", trace_id);
    
    // 1. Extract last valid signature
    let last_signature = ContextManager::extract_last_valid_signature(&original_request.messages);
    
    if let Some(ref sig) = last_signature {
        debug!("[{}] [Layer-3] Extracted signature (len: {})", trace_id, sig.len());
    }
    
    // 2. Build summary request
    let mut summary_messages = original_request.messages.clone();
    
    let signature_instruction = if let Some(ref sig) = last_signature {
        format!("\n\n**CRITICAL**: The last thinking signature is:\n```\n{}\n```\nYou MUST include this EXACTLY in the <latest_thinking_signature> section.", sig)
    } else {
        "\n\n**Note**: No thinking signature found in history. Leave <latest_thinking_signature> empty.".to_string()
    };
    
    summary_messages.push(Message {
        role: "user".to_string(),
        content: MessageContent::String(format!(
            "{}{}",
            CONTEXT_SUMMARY_PROMPT,
            signature_instruction
        )),
    });
    
    let summary_request = ClaudeRequest {
        model: INTERNAL_BACKGROUND_TASK.to_string(),
        messages: summary_messages,
        system: None,
        stream: false,
        max_tokens: Some(8000),
        temperature: Some(0.3),
        tools: None,
        thinking: None,
        metadata: None,
        top_p: None,
        top_k: None,
        output_config: None,
        size: None,
        quality: None,
    };
    
    debug!("[{}] [Layer-3] Calling {} for summary generation", trace_id, INTERNAL_BACKGROUND_TASK);
    
    // 3. Call upstream
    let xml_summary = call_gemini_sync(
        INTERNAL_BACKGROUND_TASK,
        &summary_request,
        token_manager,
        trace_id,
    ).await?;
    
    info!("[{}] [Layer-3] Generated XML summary (len: {} chars)", trace_id, xml_summary.len());
    
    // 4. Create forked conversation with summary as prefix
    let mut forked_messages = vec![
        Message {
            role: "user".to_string(),
            content: MessageContent::String(format!(
                "Context has been compressed. Here is the structured summary of our conversation history:\n\n{}",
                xml_summary
            )),
        },
        Message {
            role: "assistant".to_string(),
            content: MessageContent::String(
                "I have reviewed the compressed context summary. I understand the current state and will continue from here.".to_string()
            ),
        },
    ];
    
    // 5. Append the user's latest message
    if let Some(last_msg) = original_request.messages.last() {
        if last_msg.role == "user" {
            if !matches!(&last_msg.content, MessageContent::String(s) if s.contains(CONTEXT_SUMMARY_PROMPT)) {
                forked_messages.push(last_msg.clone());
            }
        }
    }
    
    info!(
        "[{}] [Layer-3] Fork successful: {} messages → {} messages",
        trace_id,
        original_request.messages.len(),
        forked_messages.len()
    );
    
    // 6. Return forked request
    Ok(ClaudeRequest {
        model: original_request.model.clone(),
        messages: forked_messages,
        system: original_request.system.clone(),
        stream: original_request.stream,
        max_tokens: original_request.max_tokens,
        temperature: original_request.temperature,
        tools: original_request.tools.clone(),
        thinking: original_request.thinking.clone(),
        metadata: original_request.metadata.clone(),
        top_p: original_request.top_p,
        top_k: original_request.top_k,
        output_config: original_request.output_config.clone(),
        size: original_request.size.clone(),
        quality: original_request.quality.clone(),
    })
}
