// Gemini 原生协议的发散适配器
//
// 只负责“v1internal SSE 行 → 客户端 Gemini SSE 行”的协议差异：
// 签名缓存、工具 ID 注入、图片统计、v1internal 解包、`__cloudCodeMeta` 首帧。
// 心跳、首字/状态切换/滑动超时、优雅收尾与首包错误契约全部由
// `pipeline::stream_lifecycle::run_stream_lifecycle` 统一驱动。

use bytes::Bytes;
use serde_json::Value;
use std::sync::Arc;

use crate::proxy::mappers::error_classifier::StreamErrorReport;
use crate::proxy::pipeline::stream_lifecycle::ProtocolStreamHandler;
use crate::proxy::thinking_store::TurnAccumulator;

pub struct GeminiStreamHandler {
    cloud_code_trace_id: Option<String>,
    session_id: String,
    model_name: String,
    message_count: usize,
    track_image_success: bool,
    image_success_manager: Option<Arc<crate::proxy::TokenManager>>,
    image_success_account: String,
    image_success_model: String,
    thinking_acc: Option<TurnAccumulator>,
    saw_image_data: bool,
    saw_finish_reason: bool,
    saw_function_call: bool,
    has_content: bool,
    has_thinking: bool,
}

impl GeminiStreamHandler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cloud_code_trace_id: Option<String>,
        session_id: String,
        model_name: String,
        message_count: usize,
        causal_anchor: String,
        track_image_success: bool,
        image_success_manager: Option<Arc<crate::proxy::TokenManager>>,
        image_success_account: String,
        image_success_model: String,
    ) -> Self {
        Self {
            cloud_code_trace_id,
            session_id,
            model_name,
            message_count,
            track_image_success,
            image_success_manager,
            image_success_account,
            image_success_model,
            thinking_acc: Some(TurnAccumulator::with_anchor(causal_anchor)),
            saw_image_data: false,
            saw_finish_reason: false,
            saw_function_call: false,
            has_content: false,
            has_thinking: false,
        }
    }
}

impl ProtocolStreamHandler for GeminiStreamHandler {
    fn initial_frames(&mut self) -> Vec<Bytes> {
        // 官方 Worker 会将 TraceID 作为 SSE 流的第 0 个数据包下发
        match &self.cloud_code_trace_id {
            Some(tid) => {
                let meta_pkg = serde_json::json!({ "__cloudCodeMeta": { "traceId": tid } });
                vec![Bytes::from(format!(
                    "data: {}\n\n",
                    serde_json::to_string(&meta_pkg).unwrap_or_default()
                ))]
            }
            None => Vec::new(),
        }
    }

    fn process_line(&mut self, line: &str) -> Vec<Bytes> {
        if !line.starts_with("data: ") {
            // 注释行 / 非数据行原样透传
            return vec![Bytes::from(format!("{}\n\n", line))];
        }

        let json_part = line.trim_start_matches("data: ").trim();
        if json_part == "[DONE]" {
            return vec![Bytes::from("data: [DONE]\n\n")];
        }

        let mut json: Value = match serde_json::from_str(json_part) {
            Ok(v) => v,
            Err(e) => {
                let error_json = serde_json::json!({
                    "error": {
                        "code": 502,
                        "message": format!("Gemini stream json parse error: {e}"),
                        "status": "INTERNAL"
                    }
                });
                return vec![Bytes::from(format!(
                    "data: {}\n\n",
                    serde_json::to_string(&error_json).unwrap_or_default()
                ))];
            }
        };

        if self.track_image_success && crate::proxy::pipeline::response_has_inline_image_data(&json)
        {
            self.saw_image_data = true;
        }

        // v1internal 响应可能包一层 "response"
        let parts_snapshot = json
            .get("response")
            .unwrap_or(&json)
            .get("candidates")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();

        for cand in &parts_snapshot {
            if cand.get("finishReason").is_some() || cand.get("finish_reason").is_some() {
                self.saw_finish_reason = true;
            }
            if let Some(parts) = cand
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
            {
                for part in parts {
                    let is_thought = part.get("thought").and_then(|v| v.as_bool()) == Some(true);
                    if is_thought {
                        self.has_thinking = true;
                    }
                    if part.get("functionCall").is_some() {
                        self.saw_function_call = true;
                        self.has_content = true;
                    } else if (!is_thought && part.get("text").is_some())
                        || part.get("inlineData").is_some()
                        || part.get("inline_data").is_some()
                    {
                        self.has_content = true;
                    }
                    if let Some(acc) = self.thinking_acc.as_mut() {
                        acc.ingest_part(part);
                    }
                    if let Some(sig) = part.get("thoughtSignature").and_then(|s| s.as_str()) {
                        crate::proxy::SignatureCache::global().cache_session_signature(
                            &self.session_id,
                            sig.to_string(),
                            self.message_count,
                        );
                        tracing::debug!(
                            "[Gemini-SSE] Cached session signature (len: {}) for session: {} at count {}",
                            sig.len(),
                            self.session_id,
                            self.message_count
                        );
                    }
                }
            }
        }

        // 注入工具调用 ID
        crate::proxy::mappers::gemini::wrapper::inject_ids_to_response(&mut json, &self.model_name);

        // 解包 v1internal 信封
        if let Some(inner) = json.get_mut("response").map(|v| v.take()) {
            vec![Bytes::from(format!(
                "data: {}\n\n",
                serde_json::to_string(&inner).unwrap_or_default()
            ))]
        } else {
            vec![Bytes::from(format!(
                "data: {}\n\n",
                serde_json::to_string(&json).unwrap_or_default()
            ))]
        }
    }

    fn has_content(&self) -> bool {
        self.has_content
    }

    fn has_thinking(&self) -> bool {
        self.has_thinking
    }

    fn is_terminal(&self) -> bool {
        self.saw_finish_reason
    }

    fn emit_finalize(&mut self) -> Vec<Bytes> {
        // 图片生成成功统计仅在优雅结束时提交（错误路径不会走到这里）
        if self.track_image_success && self.saw_image_data {
            if let Some(mgr) = &self.image_success_manager {
                mgr.mark_account_success(&self.image_success_account);
                mgr.clear_persisted_live_limit(
                    &self.image_success_account,
                    Some(&self.image_success_model),
                );
            }
        }
        // Gemini 正常 EOF 不额外下发 [DONE]，保持既有行为
        Vec::new()
    }

    fn emit_abnormal_finalize(&mut self, _error_report: &StreamErrorReport) -> Vec<Bytes> {
        let mut chunks = Vec::new();

        // 终态与内容防护：若上游已下发 finishReason 或从未产生任何正文/工具调用，绝不可伪造收尾
        if !self.has_content || self.saw_finish_reason {
            return chunks;
        }
        self.saw_finish_reason = true;

        // 工具调用契约保护：若本轮已有工具调用输出，终结原因保持 STOP，否则标记 MAX_TOKENS
        let finish_reason = if self.saw_function_call {
            "STOP"
        } else {
            "MAX_TOKENS"
        };

        let recovery_json = serde_json::json!({
            "candidates": [{
                "finishReason": finish_reason,
                "index": 0
            }]
        });

        chunks.push(Bytes::from(format!(
            "data: {}\n\n",
            serde_json::to_string(&recovery_json).unwrap_or_default()
        )));
        chunks
    }

    fn emit_initial_error(&mut self, error_report: &StreamErrorReport) -> Vec<Bytes> {
        let error_json = serde_json::json!({
            "error": {
                "code": 503,
                "message": error_report.client_message(),
                "status": "UNAVAILABLE",
                "function": error_report.function,
                "call_site": error_report.call_site(),
                "params": error_report.params
            }
        });
        vec![
            Bytes::from(format!(
                "data: {}\n\n",
                serde_json::to_string(&error_json).unwrap_or_default()
            )),
            Bytes::from("data: [DONE]\n\n"),
        ]
    }

    fn on_finish(&mut self) {
        // 与 Claude/OpenAI 一致：流结束（含优雅收尾）时提交思维链
        if let Some(acc) = self.thinking_acc.take() {
            acc.commit(&self.session_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn test_gemini_stream_abnormal_interruption_preserves_content_without_tampering() {
        let chunk1_json = serde_json::json!({
            "response": {
                "candidates": [{
                    "content": {
                        "parts": [{ "text": "Here is code:\n```rust\nfn main() {" }]
                    }
                }]
            }
        });

        let items: Vec<Result<Bytes, String>> = vec![
            Ok(Bytes::from(format!(
                "data: {}\n\n",
                serde_json::to_string(&chunk1_json).unwrap()
            ))),
            Err("error reading a body from connection: connection reset by peer".to_string()),
        ];

        let handler = GeminiStreamHandler::new(
            Some("trace_gemini_test".to_string()),
            "session_gemini_test".to_string(),
            "gemini-2.5-pro".to_string(),
            1,
            "anchor".to_string(),
            false,
            None,
            "acc@example.com".to_string(),
            "gemini-2.5-pro".to_string(),
        );

        let lifecycle_cfg = crate::proxy::pipeline::StreamLifecycleConfig::new(
            "gemini",
            "test_gemini_stream_abnormal_interruption",
            "test".to_string(),
        );

        let mut stream = crate::proxy::pipeline::run_stream_lifecycle(
            Box::pin(futures::stream::iter(items)),
            handler,
            lifecycle_cfg,
        );

        let mut all_chunks = Vec::new();
        while let Some(result) = stream.next().await {
            if let Ok(bytes) = result {
                all_chunks.push(String::from_utf8(bytes.to_vec()).unwrap());
            }
        }
        let output = all_chunks.join("");

        assert!(output.contains("__cloudCodeMeta"));
        assert!(output.contains("```rust\\nfn main() {"));
        // 绝不注入虚假网关告警或篡改出站正文
        assert!(!output.contains("网关提示"));
        assert!(!output.contains("Gateway Notice"));
        assert!(output.contains("MAX_TOKENS"));
    }

    #[tokio::test]
    async fn test_gemini_stream_pure_thinking_does_not_set_has_content_and_propagates_err() {
        let thinking_json = serde_json::json!({
            "response": {
                "candidates": [{
                    "content": {
                        "parts": [{ "thought": true, "text": "Thinking deeply..." }]
                    }
                }]
            }
        });

        let items: Vec<Result<Bytes, String>> = vec![
            Ok(Bytes::from(format!(
                "data: {}\n\n",
                serde_json::to_string(&thinking_json).unwrap()
            ))),
            Err("error reading a body from connection: connection reset by peer".to_string()),
        ];

        let handler = GeminiStreamHandler::new(
            Some("trace_gemini_thinking".to_string()),
            "session_gemini_thinking".to_string(),
            "gemini-2.5-pro".to_string(),
            1,
            "anchor".to_string(),
            false,
            None,
            "acc@example.com".to_string(),
            "gemini-2.5-pro".to_string(),
        );

        let lifecycle_cfg = crate::proxy::pipeline::StreamLifecycleConfig::new(
            "gemini",
            "test_gemini_thinking",
            "test".to_string(),
        );

        let mut stream = crate::proxy::pipeline::run_stream_lifecycle(
            Box::pin(futures::stream::iter(items)),
            handler,
            lifecycle_cfg,
        );

        let mut all_chunks = Vec::new();
        let mut got_err = false;
        while let Some(result) = stream.next().await {
            match result {
                Ok(bytes) => all_chunks.push(String::from_utf8(bytes.to_vec()).unwrap()),
                Err(_) => {
                    got_err = true;
                }
            }
        }
        let output = all_chunks.join("");

        // 思考阶段如实透传错误，严禁下发假正文与 MAX_TOKENS 伪造收尾
        assert!(got_err, "Pure thinking interruption must propagate error");
        assert!(!output.contains("MAX_TOKENS"));
    }

    #[tokio::test]
    async fn test_gemini_stream_completed_stream_does_not_double_finalize() {
        let completed_json = serde_json::json!({
            "response": {
                "candidates": [{
                    "content": {
                        "parts": [{ "text": "Hello world" }]
                    },
                    "finishReason": "STOP",
                    "index": 0
                }]
            }
        });

        let items: Vec<Result<Bytes, String>> = vec![
            Ok(Bytes::from(format!(
                "data: {}\n\n",
                serde_json::to_string(&completed_json).unwrap()
            ))),
            // 上游在下发带 STOP 数据包后断开
            Err("connection reset by peer".to_string()),
        ];

        let handler = GeminiStreamHandler::new(
            Some("trace_gemini_done".to_string()),
            "session_gemini_done".to_string(),
            "gemini-2.5-pro".to_string(),
            1,
            "anchor".to_string(),
            false,
            None,
            "acc@example.com".to_string(),
            "gemini-2.5-pro".to_string(),
        );

        let lifecycle_cfg = crate::proxy::pipeline::StreamLifecycleConfig::new(
            "gemini",
            "test_gemini_done",
            "test".to_string(),
        );

        let mut stream = crate::proxy::pipeline::run_stream_lifecycle(
            Box::pin(futures::stream::iter(items)),
            handler,
            lifecycle_cfg,
        );

        let mut all_chunks = Vec::new();
        while let Some(result) = stream.next().await {
            if let Ok(bytes) = result {
                all_chunks.push(String::from_utf8(bytes.to_vec()).unwrap());
            }
        }
        let output = all_chunks.join("");

        // 已正常下发 STOP 的流，严禁再次触发 emit_abnormal_finalize 附加 MAX_TOKENS
        assert!(output.contains(r#""finishReason":"STOP""#));
        assert!(!output.contains("MAX_TOKENS"));
    }
}
