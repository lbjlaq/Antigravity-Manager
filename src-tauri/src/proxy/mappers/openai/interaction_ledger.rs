use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionLedger {
    pub source: String,
    pub session_id: Option<String>,
    pub previous_response_id: Option<String>,
    pub turns: Vec<InteractionTurn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionTurn {
    pub turn_id: String,
    pub previous_turn_id: Option<String>,
    pub steps: Vec<InteractionStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionStep {
    pub step_id: String,
    pub turn_id: String,
    pub step_type: String,
    pub role: String,
    pub call_id: Option<String>,
    pub name: Option<String>,
    pub content: Option<Value>,
    pub raw_item: Value,
}

pub fn build_codex_interaction_ledger(
    input: Option<&Value>,
    instructions: Option<&str>,
    session_id: Option<String>,
    previous_response_id: Option<String>,
) -> Option<InteractionLedger> {
    let items = match input {
        Some(Value::Array(items)) => items,
        Some(Value::String(text)) => {
            let raw_item = json!({
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": text }]
            });
            return Some(ledger_from_items(
                &[raw_item],
                instructions,
                session_id,
                previous_response_id,
            ));
        }
        Some(other) => {
            let raw_item = json!({
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": other.to_string() }]
            });
            return Some(ledger_from_items(
                &[raw_item],
                instructions,
                session_id,
                previous_response_id,
            ));
        }
        None => return None,
    };

    Some(ledger_from_items(
        items,
        instructions,
        session_id,
        previous_response_id,
    ))
}

fn ledger_from_items(
    items: &[Value],
    instructions: Option<&str>,
    session_id: Option<String>,
    previous_response_id: Option<String>,
) -> InteractionLedger {
    let mut ledger = InteractionLedger {
        source: "codex_responses_input".to_string(),
        session_id,
        previous_response_id,
        turns: Vec::new(),
    };

    let mut current_turn_index: Option<usize> = None;
    let mut last_turn_id: Option<String> = None;
    let mut step_counter = 0usize;

    if let Some(text) = instructions.map(str::trim).filter(|s| !s.is_empty()) {
        let turn_index = ensure_turn(&mut ledger, &mut current_turn_index, &mut last_turn_id);
        push_step(
            &mut ledger,
            turn_index,
            &mut step_counter,
            "system_instruction",
            "system",
            None,
            None,
            Some(json!(text)),
            json!({ "type": "instructions", "content": text }),
        );
    }

    let call_id_to_name = build_call_id_to_name(items);

    for item in items {
        let item_type = item
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("");

        if item_type == "message" && role == "user" && current_turn_index.is_some() {
            current_turn_index = None;
        }

        let turn_index = ensure_turn(&mut ledger, &mut current_turn_index, &mut last_turn_id);
        let call_id = item
            .get("call_id")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("id").and_then(|v| v.as_str()))
            .map(|s| s.to_string());
        let name = canonical_tool_name(item_type, item, &call_id_to_name);
        let (step_type, step_role) = classify_step(item_type, role, name.as_deref());

        push_step(
            &mut ledger,
            turn_index,
            &mut step_counter,
            &step_type,
            &step_role,
            call_id,
            name,
            extract_step_content(item),
            item.clone(),
        );
    }

    ledger
}

fn build_call_id_to_name(items: &[Value]) -> std::collections::HashMap<String, String> {
    let mut result = std::collections::HashMap::new();
    for item in items {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if !matches!(
            item_type,
            "function_call" | "custom_tool_call" | "local_shell_call" | "web_search_call"
        ) {
            continue;
        }
        let Some(call_id) = item
            .get("call_id")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("id").and_then(|v| v.as_str()))
        else {
            continue;
        };
        let name = match item_type {
            "local_shell_call" => "shell".to_string(),
            "web_search_call" => "google_search".to_string(),
            _ => item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
        };
        result.insert(call_id.to_string(), name);
    }
    result
}

fn ensure_turn(
    ledger: &mut InteractionLedger,
    current_turn_index: &mut Option<usize>,
    last_turn_id: &mut Option<String>,
) -> usize {
    if let Some(index) = current_turn_index {
        return *index;
    }

    let turn_id = format!("turn_{}", ledger.turns.len() + 1);
    let previous_turn_id = last_turn_id.clone();
    ledger.turns.push(InteractionTurn {
        turn_id: turn_id.clone(),
        previous_turn_id,
        steps: Vec::new(),
    });
    *last_turn_id = Some(turn_id);
    let index = ledger.turns.len() - 1;
    *current_turn_index = Some(index);
    index
}

fn push_step(
    ledger: &mut InteractionLedger,
    turn_index: usize,
    step_counter: &mut usize,
    step_type: &str,
    role: &str,
    call_id: Option<String>,
    name: Option<String>,
    content: Option<Value>,
    raw_item: Value,
) {
    *step_counter += 1;
    let turn_id = ledger.turns[turn_index].turn_id.clone();
    ledger.turns[turn_index].steps.push(InteractionStep {
        step_id: format!("step_{}", step_counter),
        turn_id,
        step_type: step_type.to_string(),
        role: role.to_string(),
        call_id,
        name,
        content,
        raw_item,
    });
}

fn classify_step(item_type: &str, role: &str, name: Option<&str>) -> (String, String) {
    match item_type {
        "message" if role == "assistant" => ("model_output".to_string(), "assistant".to_string()),
        "message" if role == "system" || role == "developer" => {
            ("system_message".to_string(), role.to_string())
        }
        "message" => ("user_message".to_string(), role_or_user(role)),
        "function_call" => ("function_call".to_string(), "assistant".to_string()),
        "custom_tool_call" if matches!(name, Some("apply_patch" | "apply_patch_v2")) => {
            ("apply_patch_call".to_string(), "assistant".to_string())
        }
        "custom_tool_call" => ("custom_tool_call".to_string(), "assistant".to_string()),
        "local_shell_call" => ("local_shell_call".to_string(), "assistant".to_string()),
        "web_search_call" => ("web_search_call".to_string(), "assistant".to_string()),
        "function_call_output" => ("function_result".to_string(), "tool".to_string()),
        "custom_tool_call_output" if matches!(name, Some("apply_patch" | "apply_patch_v2")) => {
            ("apply_patch_result".to_string(), "tool".to_string())
        }
        "custom_tool_call_output" => ("custom_tool_result".to_string(), "tool".to_string()),
        _ => (item_type.to_string(), role_or_user(role)),
    }
}

fn role_or_user(role: &str) -> String {
    if role.is_empty() {
        "user".to_string()
    } else {
        role.to_string()
    }
}

fn canonical_tool_name(
    item_type: &str,
    item: &Value,
    call_id_to_name: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let raw_name = item
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    match item_type {
        "local_shell_call" => Some("shell".to_string()),
        "web_search_call" => Some("google_search".to_string()),
        "function_call_output" | "custom_tool_call_output" => item
            .get("call_id")
            .and_then(|v| v.as_str())
            .and_then(|call_id| call_id_to_name.get(call_id))
            .cloned()
            .or(raw_name),
        _ => raw_name,
    }
}

fn extract_step_content(item: &Value) -> Option<Value> {
    if let Some(content) = item.get("content") {
        return Some(content.clone());
    }
    if let Some(input) = item.get("input") {
        return Some(input.clone());
    }
    if let Some(output) = item.get("output") {
        return Some(output.clone());
    }
    if let Some(arguments) = item.get("arguments") {
        return Some(arguments.clone());
    }
    if let Some(action) = item.get("action") {
        return Some(action.clone());
    }
    None
}
