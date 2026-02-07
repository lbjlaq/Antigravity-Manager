//! Unit tests for streaming module.

use super::*;
use super::state::SignatureManager;
use crate::proxy::mappers::claude::models::*;
use serde_json::json;

#[test]
fn test_signature_manager() {
    let mut mgr = SignatureManager::new();
    assert!(!mgr.has_pending());

    mgr.store(Some("sig123".to_string()));
    assert!(mgr.has_pending());

    let sig = mgr.consume();
    assert_eq!(sig, Some("sig123".to_string()));
    assert!(!mgr.has_pending());
}

#[test]
fn test_streaming_state_emit() {
    let state = StreamingState::new();
    let chunk = state.emit("test_event", json!({"foo": "bar"}));

    let s = String::from_utf8(chunk.to_vec()).unwrap();
    assert!(s.contains("event: test_event"));
    assert!(s.contains("\"foo\":\"bar\""));
}

#[test]
fn test_process_function_call_deltas() {
    let mut state = StreamingState::new();
    let mut processor = PartProcessor::new(&mut state);

    let fc = FunctionCall {
        name: "test_tool".to_string(),
        args: Some(json!({"arg": "value"})),
        id: Some("call_123".to_string()),
    };

    // Create a dummy GeminiPart with function_call
    let part = GeminiPart {
        text: None,
        function_call: Some(fc),
        inline_data: None,
        thought: None,
        thought_signature: None,
        function_response: None,
    };

    let chunks = processor.process(&part);
    let output = chunks
        .iter()
        .map(|b| String::from_utf8(b.to_vec()).unwrap())
        .collect::<Vec<_>>()
        .join("");

    // Verify sequence:
    // 1. content_block_start with empty input
    assert!(output.contains(r#""type":"content_block_start""#));
    assert!(output.contains(r#""name":"test_tool""#));
    assert!(output.contains(r#""input":{}"#));

    // 2. input_json_delta with serialized args
    assert!(output.contains(r#""type":"content_block_delta""#));
    assert!(output.contains(r#""type":"input_json_delta""#));
    // partial_json should contain escaped JSON string
    assert!(output.contains(r#"partial_json":"{\"arg\":\"value\"}"#));

    // 3. content_block_stop
    assert!(output.contains(r#""type":"content_block_stop""#));
}
