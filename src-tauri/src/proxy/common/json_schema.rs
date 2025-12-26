use serde_json::Value;

/// 递归清理 JSON Schema 以符合 Gemini 接口要求
/// 
/// 1. 移除不支持的字段: $schema, additionalProperties, format, default, uniqueItems, validation fields
/// 2. 处理联合类型: ["string", "null"] -> "string"
/// 3. 将 type 字段的值转换为大写 (Gemini v1internal 要求)
/// 4. 移除数字校验字段: multipleOf, exclusiveMinimum, exclusiveMaximum 等
pub fn clean_json_schema(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // 1. 移除不支持的字段
            let fields_to_remove = [
                "$schema",
                "additionalProperties",
                "format",
                "default",
                "uniqueItems",
                // Claude/JSONSchema extensions not accepted by Gemini
                "enumCaseInsensitive",
                "enumNormalizeWhitespace",
                "minLength",
                "maxLength",
                "minimum",
                "maximum",
                "exclusiveMinimum",
                "exclusiveMaximum",
                "multipleOf",
                "minItems",
                "maxItems",
                "pattern",
                "const",
                "minProperties",
                "maxProperties",
                "propertyNames",
                "patternProperties",
                "contains",
                "minContains",
                "maxContains",
                "if",
                "then",
                "else",
                "not",
            ];

            for field in fields_to_remove {
                map.remove(field);
            }

            // 2. 处理 type 字段 (Union Types -> Primary Type + Uppercase)
            if let Some(type_val) = map.get_mut("type") {
                match type_val {
                    Value::String(s) => {
                        *type_val = Value::String(s.to_uppercase());
                    }
                    Value::Array(arr) => {
                        // Handle ["string", "null"] -> select first non-null
                        let mut selected_type = "STRING".to_string(); // Default fallback
                        for item in arr {
                            if let Value::String(s) = item {
                                if s != "null" {
                                    selected_type = s.to_uppercase();
                                    break;
                                }
                            }
                        }
                        *type_val = Value::String(selected_type);
                    }
                    _ => {}
                }
            }

            // 3. 递归处理 properties, items, allOf, anyOf, oneOf 等
            for (key, v) in map.iter_mut() {
                // 特殊处理 properties 和 items 的子节点
                if key == "properties" {
                    if let Some(props_obj) = v.as_object_mut() {
                        for (_, prop_val) in props_obj.iter_mut() {
                            clean_json_schema(prop_val);
                        }
                    }
                } else if key == "items" {
                    clean_json_schema(v);
                } else if key == "allOf" || key == "anyOf" || key == "oneOf" {
                    if let Some(arr) = v.as_array_mut() {
                        for item in arr {
                            clean_json_schema(item);
                        }
                    }
                }
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                clean_json_schema(v);
            }
        }
        _ => {}
    }
}
