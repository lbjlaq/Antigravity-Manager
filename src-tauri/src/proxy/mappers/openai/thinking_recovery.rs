use serde_json::Value;

pub fn strip_all_thinking_blocks(contents: Vec<Value>) -> Vec<Value> {
    contents
        .into_iter()
        .map(|mut content| {
            if let Some(parts) = content.get_mut("parts").and_then(|v| v.as_array_mut()) {
                parts.retain(|part| {
                    !part
                        .get("thought")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                });
            }
            content
        })
        .filter(|msg| !msg["parts"].as_array().map(|a| a.is_empty()).unwrap_or(true))
        .collect()
}
