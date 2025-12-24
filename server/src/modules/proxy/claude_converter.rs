use serde::Serialize;
use serde_json::Value;

#[derive(Debug, PartialEq, Clone, Copy)]
#[allow(dead_code)]
pub enum ResponseType {
    None = 0,
    Thinking = 1,
    Text = 2,
    Tool = 3,
}

#[derive(Serialize)]
pub struct StreamEvent {
    pub event: String, // event name
    pub data: String, // JSON data string
}

pub struct ClaudeStreamConverter {
    pub response_index: usize,
    current_type: ResponseType,
    pub has_content: bool,
}

impl ClaudeStreamConverter {
    pub fn new() -> Self {
        Self {
            response_index: 0,
            current_type: ResponseType::None,
            has_content: false,
        }
    }

    /// Process a Gemini/OpenAI-format chunk and return a list of Anthropic SSE events.
    pub fn process_chunk(&mut self, json_chunk: &Value) -> Vec<StreamEvent> {
        let mut events = Vec::new();

        // Safety check for empty choices (should be handled by pre-check, but just in case)
        let choices = match json_chunk.get("choices").and_then(|c| c.as_array()) {
            Some(arr) if !arr.is_empty() => arr,
            _ => return events, // Return empty if no choices
        };

        let choice = &choices[0];
        let delta = &choice["delta"];
        
        let delta_content = delta.get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
            
        // Check for thinking fields
        let is_thought = delta.get("thought")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
            
        let thought_signature = delta.get("thoughtSignature")
            .and_then(|v| v.as_str());

        // Update has_content status
        if !delta_content.is_empty() || is_thought || thought_signature.is_some() {
            self.has_content = true;
        }

        // --- State Machine Logic ---

        // 1. Handle Thinking (or Thought Signature)
        if is_thought || thought_signature.is_some() {
            // Close existing Text block if open
            if self.current_type == ResponseType::Text {
                events.push(self.create_event("content_block_stop", serde_json::json!({
                    "type": "content_block_stop",
                    "index": self.response_index
                })));
                self.response_index += 1;
                self.current_type = ResponseType::None;
            }

            // Open Thinking block if not open
            if self.current_type == ResponseType::None {
                events.push(self.create_event("content_block_start", serde_json::json!({
                    "type": "content_block_start",
                    "index": self.response_index,
                    "content_block": { "type": "text", "text": "" }
                })));
                self.current_type = ResponseType::Thinking;
            }

            // Send thought signature delta
            // if let Some(sig) = thought_signature {
            //    // text 类型的 block 不支持 signature_delta，直接忽略或记录日志
            //    tracing::debug!("(Converter) Skipping signature_delta for text block: {}", sig);
            // }

            // Send thinking content delta as TEXT
            if !delta_content.is_empty() {
                events.push(self.create_event("content_block_delta", serde_json::json!({
                    "type": "content_block_delta",
                    "index": self.response_index,
                    "delta": { "type": "text_delta", "text": delta_content }
                })));
            }
        } 
        // 2. Handle Regular Text
        else if !delta_content.is_empty() {
            // Close existing Thinking block if open
            if self.current_type == ResponseType::Thinking {
                events.push(self.create_event("content_block_stop", serde_json::json!({
                    "type": "content_block_stop",
                    "index": self.response_index
                })));
                self.response_index += 1;
                self.current_type = ResponseType::None;
            }

            // Open Text block if not open
            if self.current_type == ResponseType::None {
                events.push(self.create_event("content_block_start", serde_json::json!({
                   "type": "content_block_start",
                   "index": self.response_index,
                   "content_block": { "type": "text", "text": "" } 
                })));
                self.current_type = ResponseType::Text;
            }

            // Send text content delta
            events.push(self.create_event("content_block_delta", serde_json::json!({
                "type": "content_block_delta",
                "index": self.response_index,
                "delta": { "type": "text_delta", "text": delta_content }
            })));
        }

        // 3. Handle Stop Reason (if present in this chunk)
        if let Some(reason_str) = choice.get("finish_reason").and_then(|v| v.as_str()) {
             // Close any open block first
            if self.current_type != ResponseType::None {
                 events.push(self.create_event("content_block_stop", serde_json::json!({
                    "type": "content_block_stop",
                    "index": self.response_index
                })));
                self.response_index += 1;
                self.current_type = ResponseType::None;
            }
            
            // Map finish reason
             let stop_reason = match reason_str {
                "length" | "MAX_TOKENS" => "max_tokens",
                "stop" | "STOP" => "end_turn",
                "tool_calls" | "function_call" => "tool_use", 
                _ => "end_turn"
            };

            // Send message_delta with stop reason
             events.push(self.create_event("message_delta", serde_json::json!({
                "type": "message_delta",
                "delta": { "stop_reason": stop_reason, "stop_sequence": null }, 
                "usage": { "output_tokens": 0 } // usage usually updated in finish(), this is just for stop signal
            })));
            
             // Send final message_stop
            events.push(self.create_event("message_stop", serde_json::json!({
                 "type": "message_stop"
            })));
        }

        events
    }

    /// Create a message_start event helper
    pub fn create_message_start(msg_id: &str, model: &str) -> StreamEvent {
         let data = serde_json::json!({
            "type": "message_start",
            "message": {
                "id": msg_id,
                "type": "message",
                "role": "assistant",
                "model": model,
                "content": [],
                "stop_reason": null,
                "stop_sequence": null,
                "usage": { "input_tokens": 0, "output_tokens": 0 }
            }
        });
        StreamEvent {
            event: "message_start".to_string(),
            data: data.to_string(),
        }
    }

    fn create_event(&self, event_name: &str, data: Value) -> StreamEvent {
        StreamEvent {
            event: event_name.to_string(),
            data: data.to_string(),
        }
    }
}
