use serde_json::Value;

/// 递归清理 JSON Schema 以符合 Gemini 接口要求
///
/// 1. [New] 展开 $ref 和 $defs: 将引用替换为实际定义，解决 Gemini 不支持 $ref 的问题
/// 2. 移除不支持的字段: $schema, additionalProperties, format, default, uniqueItems, validation fields
/// 3. 处理联合类型: ["string", "null"] -> "string"
/// 4. 将 type 字段的值转换为大写 (Gemini v1internal 要求)
/// 5. 移除数字校验字段: multipleOf, exclusiveMinimum, exclusiveMaximum 等
pub fn clean_json_schema(value: &mut Value) {
    // 0. 预处理：收集所有 $defs 定义（支持嵌套）
    let defs = collect_all_defs(value);

    // 1. 展开所有 $ref 引用
    if !defs.is_empty() {
        flatten_refs_with_defs(value, &defs);
    }

    // 2. 移除所有残留的 $ref（无法解析的引用）
    remove_unresolved_refs(value);

    // 3. 递归清理
    clean_json_schema_recursive(value);
}

/// 递归收集所有 $defs 和 definitions
fn collect_all_defs(value: &Value) -> serde_json::Map<String, Value> {
    let mut all_defs = serde_json::Map::new();
    collect_defs_recursive(value, &mut all_defs);
    all_defs
}

fn collect_defs_recursive(value: &Value, defs: &mut serde_json::Map<String, Value>) {
    if let Value::Object(map) = value {
        // 收集当前层的 $defs
        if let Some(Value::Object(d)) = map.get("$defs") {
            for (k, v) in d {
                defs.insert(k.clone(), v.clone());
            }
        }
        if let Some(Value::Object(d)) = map.get("definitions") {
            for (k, v) in d {
                defs.insert(k.clone(), v.clone());
            }
        }

        // 递归子节点
        for (_, v) in map {
            collect_defs_recursive(v, defs);
        }
    } else if let Value::Array(arr) = value {
        for item in arr {
            collect_defs_recursive(item, defs);
        }
    }
}

/// 使用收集的 defs 展开所有 $ref
fn flatten_refs_with_defs(value: &mut Value, defs: &serde_json::Map<String, Value>) {
    match value {
        Value::Object(map) => {
            // 检查并替换 $ref
            if let Some(Value::String(ref_path)) = map.remove("$ref") {
                let ref_name = ref_path.split('/').last().unwrap_or(&ref_path);

                if let Some(def_schema) = defs.get(ref_name) {
                    if let Value::Object(def_map) = def_schema {
                        for (k, v) in def_map {
                            map.entry(k.clone()).or_insert_with(|| v.clone());
                        }
                    }
                }
            }

            // 移除 $defs 和 definitions（已经收集过了）
            map.remove("$defs");
            map.remove("definitions");

            // 递归处理子节点
            for (_, v) in map.iter_mut() {
                flatten_refs_with_defs(v, defs);
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                flatten_refs_with_defs(item, defs);
            }
        }
        _ => {}
    }
}

/// 移除所有无法解析的 $ref（作为最后的清理步骤）
fn remove_unresolved_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // 移除 $ref
            map.remove("$ref");
            map.remove("$defs");
            map.remove("definitions");

            // 递归处理子节点
            for (_, v) in map.iter_mut() {
                remove_unresolved_refs(v);
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                remove_unresolved_refs(item);
            }
        }
        _ => {}
    }
}

fn clean_json_schema_recursive(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // 1. 先递归处理所有子节点，确保嵌套结构被正确清理
            for v in map.values_mut() {
                clean_json_schema_recursive(v);
            }

            // 2. 收集并处理校验字段 (Soft-Remove with Type Check & Unwrapping)
            let mut constraints = Vec::new();
            
            // String 类型校验 (pattern): 必须是 String，否则可能是属性定义
            let string_validations = [("pattern", "pattern")];
            for (field, label) in string_validations {
                if let Some(val) = map.remove(field) {
                    if let Value::String(s) = val {
                        constraints.push(format!("{}: {}", label, s));
                    } else {
                        // 不是 String (例如是 Object 类型的属性定义)，放回去
                        map.insert(field.to_string(), val);
                    }
                }
            }

            // Number 类型校验
            let number_validations = [
                ("minLength", "minLen"), ("maxLength", "maxLen"),
                ("minimum", "min"), ("maximum", "max"),
                ("minItems", "minItems"), ("maxItems", "maxItems"),
                ("exclusiveMinimum", "exclMin"), ("exclusiveMaximum", "exclMax"),
                ("multipleOf", "multipleOf"),
            ];
            for (field, label) in number_validations {
                if let Some(val) = map.remove(field) {
                    if val.is_number() {
                        constraints.push(format!("{}: {}", label, val));
                    } else {
                        // 不是 Number，放回去
                        map.insert(field.to_string(), val);
                    }
                }
            }

            // 3. 将约束信息追加到描述
            if !constraints.is_empty() {
                let suffix = format!(" [Validation: {}]", constraints.join(", "));
                let desc = map.entry("description".to_string()).or_insert_with(|| Value::String("".to_string()));
                if let Value::String(s) = desc {
                    s.push_str(&suffix);
                }
            }

            // 4. 移除其他会干扰上游的非标准/冲突字段
            let other_fields_to_remove = [
                "$schema",
                "additionalProperties",
                "enumCaseInsensitive",
                "enumNormalizeWhitespace",
                "uniqueItems",
                "format",
                "default",
                // MCP 工具常用但 Gemini 不支持的高级字段
                "propertyNames",
                "const",
                "anyOf",
                "oneOf",
                "allOf",
                "not",
                "if",
                "then",
                "else",
            ];
            for field in other_fields_to_remove {
                map.remove(field);
            }

            // 5. 同步 required 数组：移除不存在于 properties 中的属性名
            // 这是为了解决 $ref 展开或 anyOf/oneOf 移除后，required 引用了不存在属性的问题
            if let (Some(Value::Array(required)), Some(Value::Object(properties))) =
                (map.get("required"), map.get("properties"))
            {
                let valid_props: std::collections::HashSet<&str> = properties
                    .keys()
                    .map(|s| s.as_str())
                    .collect();

                let cleaned_required: Vec<Value> = required
                    .iter()
                    .filter(|r| {
                        if let Value::String(prop_name) = r {
                            valid_props.contains(prop_name.as_str())
                        } else {
                            false
                        }
                    })
                    .cloned()
                    .collect();

                // 更新或移除 required 字段
                if cleaned_required.is_empty() {
                    map.remove("required");
                } else if cleaned_required.len() != required.len() {
                    map.insert("required".to_string(), Value::Array(cleaned_required));
                }
            }

            // 6. 处理 type 字段 (Gemini Protobuf 不支持数组类型，强制降级)
            if let Some(type_val) = map.get_mut("type") {
                match type_val {
                    Value::String(s) => {
                        *type_val = Value::String(s.to_lowercase());
                    }
                    Value::Array(arr) => {
                        // Handle ["string", "null"] -> select first non-null string
                        // 任何数组类型都必须降级为单一类型
                        let mut selected_type = "string".to_string(); 
                        for item in arr {
                            if let Value::String(s) = item {
                                if s != "null" {
                                    selected_type = s.to_lowercase();
                                    break;
                                }
                            }
                        }
                        *type_val = Value::String(selected_type);
                    }
                    _ => {}
                }
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                clean_json_schema_recursive(v);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_clean_json_schema_draft_2020_12() {
        let mut schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "minLength": 1,
                    "format": "city"
                },
                // 模拟属性名冲突：pattern 是一个 Object 属性，不应被移除
                "pattern": {
                    "type": "object",
                    "properties": {
                        "regex": { "type": "string", "pattern": "^[a-z]+$" }
                    }
                },
                "unit": {
                    "type": ["string", "null"],
                    "default": "celsius"
                }
            },
            "required": ["location"]
        });

        clean_json_schema(&mut schema);

        // 1. 验证类型保持小写
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["location"]["type"], "string");

        // 2. 验证标准字段被转换并移动到描述 (Advanced Soft-Remove)
        assert!(schema["properties"]["location"].get("minLength").is_none());
        assert!(schema["properties"]["location"]["description"].as_str().unwrap().contains("minLen: 1"));

        // 3. 验证名为 "pattern" 的属性未被误删
        assert!(schema["properties"].get("pattern").is_some());
        assert_eq!(schema["properties"]["pattern"]["type"], "object");

        // 4. 验证内部的 pattern 校验字段被正确移除并转为描述
        assert!(schema["properties"]["pattern"]["properties"]["regex"].get("pattern").is_none());
        assert!(schema["properties"]["pattern"]["properties"]["regex"]["description"].as_str().unwrap().contains("pattern: ^[a-z]+$"));

        // 5. 验证联合类型被降级为单一类型 (Protobuf 兼容性)
        assert_eq!(schema["properties"]["unit"]["type"], "string");
        
        // 6. 验证元数据字段被移除
        assert!(schema.get("$schema").is_none());
    }

    #[test]
    fn test_type_fallback() {
        // Test ["string", "null"] -> "string"
        let mut s1 = json!({"type": ["string", "null"]});
        clean_json_schema(&mut s1);
        assert_eq!(s1["type"], "string");

        // Test ["integer", "null"] -> "integer" (and lowercase check if needed, though usually integer)
        let mut s2 = json!({"type": ["integer", "null"]});
        clean_json_schema(&mut s2);
        assert_eq!(s2["type"], "integer");
    }

    #[test]
    fn test_flatten_refs() {
        let mut schema = json!({
            "$defs": {
                "Address": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    }
                }
            },
            "properties": {
                "home": { "$ref": "#/$defs/Address" }
            }
        });

        clean_json_schema(&mut schema);

        // 验证引用被展开且类型转为小写
        assert_eq!(schema["properties"]["home"]["type"], "object");
        assert_eq!(schema["properties"]["home"]["properties"]["city"]["type"], "string");
    }

    #[test]
    fn test_required_sync_with_properties() {
        // 测试 required 数组与 properties 不一致时的清理
        let mut schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name", "non_existent_field", "another_missing"]
        });

        clean_json_schema(&mut schema);

        // 验证 required 只保留存在于 properties 中的字段
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "name");
    }

    #[test]
    fn test_required_removed_when_all_invalid() {
        // 当所有 required 字段都不存在时，应该移除 required
        let mut schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["missing1", "missing2"]
        });

        clean_json_schema(&mut schema);

        // required 字段应被完全移除
        assert!(schema.get("required").is_none());
    }
}
