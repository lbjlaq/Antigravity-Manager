use serde_json::Value;

/// 递归清理 JSON Schema 以符合 Gemini 接口要求
///
/// 1. [New] 展开 $ref 和 $defs: 将引用替换为实际定义，解决 Gemini 不支持 $ref 的问题
/// 2. 移除不支持的字段: $schema, additionalProperties, format, default, uniqueItems, validation fields
/// 3. 处理联合类型: ["string", "null"] -> "string"
/// 4. 将 type 字段的值转换为大写 (Gemini v1internal 要求)
/// 5. 移除数字校验字段: multipleOf, exclusiveMinimum, exclusiveMaximum 等
pub fn clean_json_schema(value: &mut Value) {
    // 0. 预处理：展开 $ref (Schema Flattening)
    if let Value::Object(map) = value {
        let mut defs = serde_json::Map::new();
        // 提取 $defs 或 definitions
        if let Some(Value::Object(d)) = map.remove("$defs") {
            defs.extend(d);
        }
        if let Some(Value::Object(d)) = map.remove("definitions") {
            defs.extend(d);
        }

        if !defs.is_empty() {
            // 递归替换引用
            flatten_refs(map, &defs);
        }
    }

    // 递归清理
    clean_json_schema_recursive(value);
}

/// 递归展开 $ref
fn flatten_refs(map: &mut serde_json::Map<String, Value>, defs: &serde_json::Map<String, Value>) {
    // 检查并替换 $ref
    if let Some(Value::String(ref_path)) = map.remove("$ref") {
        // 解析引用名 (例如 #/$defs/MyType -> MyType)
        let ref_name = ref_path.split('/').last().unwrap_or(&ref_path);

        if let Some(def_schema) = defs.get(ref_name) {
            // 将定义的内容合并到当前 map
            if let Value::Object(def_map) = def_schema {
                for (k, v) in def_map {
                    // 仅当当前 map 没有该 key 时才插入 (避免覆盖)
                    // 但通常 $ref 节点不应该有其他属性
                    map.entry(k.clone()).or_insert_with(|| v.clone());
                }

                // 递归处理刚刚合并进来的内容中可能包含的 $ref
                // 注意：这里可能会无限递归如果存在循环引用，但工具定义通常是 DAG
                flatten_refs(map, defs);
            }
        }
    }

    // 遍历子节点
    for (_, v) in map.iter_mut() {
        if let Value::Object(child_map) = v {
            flatten_refs(child_map, defs);
        } else if let Value::Array(arr) = v {
            for item in arr {
                if let Value::Object(item_map) = item {
                    flatten_refs(item_map, defs);
                }
            }
        }
    }
}

fn clean_json_schema_recursive(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // 1. [CRITICAL] 深度递归处理：必须遍历当前对象的所有字段名对应的 Value
            // 解决 properties/items 之外的 definitions、anyOf、allOf 等结构的清理
            for v in map.values_mut() {
                clean_json_schema_recursive(v);
            }

            // 2. 收集并处理校验字段 (Migration logic: 将约束降级为描述中的 Hint)
            let mut constraints = Vec::new();

            // 待迁移的约束黑名单
            let validation_fields = [
                ("pattern", "pattern"),
                ("minLength", "minLen"),
                ("maxLength", "maxLen"),
                ("minimum", "min"),
                ("maximum", "max"),
                ("minItems", "minItems"),
                ("maxItems", "maxItems"),
                ("exclusiveMinimum", "exclMin"),
                ("exclusiveMaximum", "exclMax"),
                ("multipleOf", "multipleOf"),
                ("format", "format"),
            ];

            for (field, label) in validation_fields {
                if let Some(val) = map.remove(field) {
                    // 仅当值是简单类型时才迁移
                    if val.is_string() || val.is_number() || val.is_boolean() {
                        constraints.push(format!("{}: {}", label, val));
                    }
                }
            }

            // 3. 将约束信息追加到描述
            if !constraints.is_empty() {
                let suffix = format!(" [Constraint: {}]", constraints.join(", "));
                let desc_val = map
                    .entry("description".to_string())
                    .or_insert_with(|| Value::String("".to_string()));
                if let Value::String(s) = desc_val {
                    s.push_str(&suffix);
                }
            }

            // 4. 彻底物理移除干扰生成的"硬项"黑色名单 (Hard Blacklist)
            let hard_remove_fields = [
                "$schema",
                "$id", // [NEW] JSON Schema identifier
                "additionalProperties",
                "enumCaseInsensitive",
                "enumNormalizeWhitespace",
                "uniqueItems",
                "default",
                "const",
                "examples",
                "propertyNames",
                "anyOf",
                "oneOf",
                "allOf",
                "not",
                "if",
                "then",
                "else",
                "dependencies",
                "dependentSchemas",
                "dependentRequired",
                "cache_control",
                "contentEncoding",  // [NEW] base64 encoding hint
                "contentMediaType", // [NEW] MIME type hint
                "deprecated",       // [NEW] Gemini doesn't understand this
                "readOnly",         // [NEW]
                "writeOnly",        // [NEW]
            ];
            for field in hard_remove_fields {
                map.remove(field);
            }

            // [NEW FIX] 确保 required 中的字段一定在 properties 中存在
            // Gemini 严格校验：required 中的字段如果不在 properties 中定义，会报 INVALID_ARGUMENT
            // Refactored to avoid double borrow (mutable map vs immutable get("properties"))
            let valid_prop_keys: Option<std::collections::HashSet<String>> = map
                .get("properties")
                .and_then(|p| p.as_object())
                .map(|obj| obj.keys().cloned().collect());

            if let Some(required_val) = map.get_mut("required") {
                if let Some(req_arr) = required_val.as_array_mut() {
                    if let Some(keys) = &valid_prop_keys {
                        req_arr.retain(|k| {
                            if let Some(k_str) = k.as_str() {
                                keys.contains(k_str)
                            } else {
                                false
                            }
                        });
                    } else {
                        // 如果没有 properties，required 应该是空的
                        req_arr.clear();
                    }
                }
            }

            // 5. 处理 type 字段 (Gemini 要求单字符串且小写)
            if let Some(type_val) = map.get_mut("type") {
                match type_val {
                    Value::String(s) => {
                        *type_val = Value::String(s.to_lowercase());
                    }
                    Value::Array(arr) => {
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
        assert!(schema["properties"]["location"]["description"]
            .as_str()
            .unwrap()
            .contains("minLen: 1"));

        // 3. 验证名为 "pattern" 的属性未被误删
        assert!(schema["properties"].get("pattern").is_some());
        assert_eq!(schema["properties"]["pattern"]["type"], "object");

        // 4. 验证内部的 pattern 校验字段被正确移除并转为描述
        assert!(schema["properties"]["pattern"]["properties"]["regex"]
            .get("pattern")
            .is_none());
        assert!(
            schema["properties"]["pattern"]["properties"]["regex"]["description"]
                .as_str()
                .unwrap()
                .contains("pattern: ^[a-z]+$")
        );

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
        assert_eq!(
            schema["properties"]["home"]["properties"]["city"]["type"],
            "string"
        );
    }

    #[test]
    fn test_clean_json_schema_missing_required() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "existing_prop": { "type": "string" }
            },
            "required": ["existing_prop", "missing_prop"]
        });

        clean_json_schema(&mut schema);

        // 验证 missing_prop 被从 required 中移除
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0].as_str().unwrap(), "existing_prop");
    }
}
