//! Unit tests for JSON Schema cleaning utilities.

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
            // Simulate property name conflict: pattern is an Object property, should not be removed
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

    // 1. Verify type remains lowercase
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["location"]["type"], "string");

    // 2. Verify standard fields are removed and converted to description (Robust Constraint Migration)
    assert!(schema["properties"]["location"].get("minLength").is_none());
    assert!(schema["properties"]["location"].get("format").is_none());
    assert!(schema["properties"]["location"]["description"]
        .as_str()
        .unwrap()
        .contains("[Constraint: minLen: 1, format: city]"));

    // 3. Verify property named "pattern" is not mistakenly deleted
    assert!(schema["properties"].get("pattern").is_some());
    assert_eq!(schema["properties"]["pattern"]["type"], "object");

    // 4. Verify inner pattern validation field is removed and converted to description
    assert!(schema["properties"]["pattern"]["properties"]["regex"]
        .get("pattern")
        .is_none());
    assert!(
        schema["properties"]["pattern"]["properties"]["regex"]["description"]
            .as_str()
            .unwrap()
            .contains("[Constraint: pattern: ^[a-z]+$]")
    );

    // 5. Verify union type is downgraded to single type (Protobuf compatibility)
    assert_eq!(schema["properties"]["unit"]["type"], "string");

    // 6. Verify metadata fields are removed
    assert!(schema.get("$schema").is_none());
}

#[test]
fn test_type_fallback() {
    // Test ["string", "null"] -> "string"
    let mut s1 = json!({"type": ["string", "null"]});
    clean_json_schema(&mut s1);
    assert_eq!(s1["type"], "string");

    // Test ["integer", "null"] -> "integer"
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

    // Verify reference is expanded and type is lowercase
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

    // Verify missing_prop is removed from required
    let required = schema["required"].as_array().unwrap();
    assert_eq!(required.len(), 1);
    assert_eq!(required[0].as_str().unwrap(), "existing_prop");
}

#[test]
fn test_anyof_type_extraction() {
    // Test FastMCP style Optional[str] schema
    let mut schema = json!({
        "type": "object",
        "properties": {
            "testo": {
                "anyOf": [
                    {"type": "string"},
                    {"type": "null"}
                ],
                "default": null,
                "title": "Testo"
            },
            "importo": {
                "anyOf": [
                    {"type": "number"},
                    {"type": "null"}
                ],
                "default": null,
                "title": "Importo"
            },
            "attivo": {
                "type": "boolean",
                "title": "Attivo"
            }
        }
    });

    clean_json_schema(&mut schema);

    // Verify anyOf is removed
    assert!(schema["properties"]["testo"].get("anyOf").is_none());
    assert!(schema["properties"]["importo"].get("anyOf").is_none());

    // Verify type is correctly extracted
    assert_eq!(schema["properties"]["testo"]["type"], "string");
    assert_eq!(schema["properties"]["importo"]["type"], "number");
    assert_eq!(schema["properties"]["attivo"]["type"], "boolean");

    // Verify default is removed (outside whitelist)
    assert!(schema["properties"]["testo"].get("default").is_none());
}

#[test]
fn test_oneof_type_extraction() {
    let mut schema = json!({
        "properties": {
            "value": {
                "oneOf": [
                    {"type": "integer"},
                    {"type": "null"}
                ]
            }
        }
    });

    clean_json_schema(&mut schema);

    assert!(schema["properties"]["value"].get("oneOf").is_none());
    assert_eq!(schema["properties"]["value"]["type"], "integer");
}

#[test]
fn test_existing_type_preserved() {
    let mut schema = json!({
        "properties": {
            "name": {
                "type": "string",
                "anyOf": [
                    {"type": "number"}
                ]
            }
        }
    });

    clean_json_schema(&mut schema);

    // type already exists, should not be overwritten by anyOf type
    assert_eq!(schema["properties"]["name"]["type"], "string");
    assert!(schema["properties"]["name"].get("anyOf").is_none());
}

#[test]
fn test_issue_815_anyof_properties_preserved() {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "config": {
                "anyOf": [
                    {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "recursive": { "type": "boolean" }
                        },
                        "required": ["path"]
                    },
                    { "type": "null" }
                ]
            }
        }
    });

    clean_json_schema(&mut schema);

    let config = &schema["properties"]["config"];

    // 1. Verify type is extracted
    assert_eq!(config["type"], "object");

    // 2. Verify anyOf internal properties are merged up
    assert!(config.get("properties").is_some());
    assert_eq!(config["properties"]["path"]["type"], "string");
    assert_eq!(config["properties"]["recursive"]["type"], "boolean");

    // 3. Verify required is merged up
    let req = config["required"].as_array().unwrap();
    assert!(req.iter().any(|v| v == "path"));

    // 4. Verify anyOf field itself is removed
    assert!(config.get("anyOf").is_none());

    // 5. Verify no reason injection (because we preserved properties)
    assert!(config["properties"].get("reason").is_none());
}

#[test]
fn test_clean_json_schema_on_non_schema_object() {
    // Simulate half-converted functionCall object from request.rs
    let mut tool_call = json!({
        "functionCall": {
            "name": "local_shell_call",
            "args": { "command": ["ls"] },
            "id": "call_123"
        }
    });

    // Call cleaning logic
    clean_json_schema(&mut tool_call);

    // Verify: these non-Schema fields should not be removed (don't match looks_like_schema)
    let fc = &tool_call["functionCall"];
    assert_eq!(fc["name"], "local_shell_call");
    assert_eq!(fc["args"]["command"][0], "ls");
    assert_eq!(fc["id"], "call_123");
}

#[test]
fn test_nullable_handling_with_description() {
    let mut schema = json!({
        "type": ["string", "null"],
        "description": "User name"
    });

    clean_json_schema(&mut schema);

    // Verify type is downgraded and description is appended with (nullable)
    assert_eq!(schema["type"], "string");
    assert!(schema["description"]
        .as_str()
        .unwrap()
        .contains("User name"));
    assert!(schema["description"]
        .as_str()
        .unwrap()
        .contains("(nullable)"));
}

#[test]
fn test_clean_anyof_with_propertynames() {
    let mut schema = json!({
        "properties": {
            "config": {
                "anyOf": [
                    {
                        "type": "object",
                        "propertyNames": {"pattern": "^[a-z]+$"},
                        "properties": {
                            "key": {"type": "string"}
                        }
                    },
                    {"type": "null"}
                ]
            }
        }
    });

    clean_json_schema(&mut schema);

    // Verify anyOf is removed (merged)
    let config = &schema["properties"]["config"];
    assert!(config.get("anyOf").is_none());

    // Verify propertyNames is removed
    assert!(config.get("propertyNames").is_none());

    // Verify merged properties exist without propertyNames
    assert!(config.get("properties").is_some());
    assert_eq!(config["properties"]["key"]["type"], "string");
}

#[test]
fn test_clean_items_array_with_const() {
    let mut schema = json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "status": {
                    "const": "active",
                    "type": "string"
                }
            }
        }
    });

    clean_json_schema(&mut schema);

    // Verify const is removed
    let status = &schema["items"]["properties"]["status"];
    assert!(status.get("const").is_none());

    // Verify type still exists
    assert_eq!(status["type"], "string");
}

#[test]
fn test_deep_nested_array_cleaning() {
    let mut schema = json!({
        "properties": {
            "data": {
                "anyOf": [
                    {
                        "type": "array",
                        "items": {
                            "anyOf": [
                                {
                                    "type": "object",
                                    "propertyNames": {"maxLength": 10},
                                    "const": "test",
                                    "properties": {
                                        "name": {"type": "string"}
                                    }
                                },
                                {"type": "null"}
                            ]
                        }
                    }
                ]
            }
        }
    });

    clean_json_schema(&mut schema);

    // Verify deeply nested illegal fields are all removed
    let data = &schema["properties"]["data"];

    // anyOf should be merged and removed
    assert!(data.get("anyOf").is_none());

    // Verify no propertyNames and const escaped to top level
    assert!(data.get("propertyNames").is_none());
    assert!(data.get("const").is_none());

    // Verify structure is correctly preserved
    assert_eq!(data["type"], "array");
    if let Some(items) = data.get("items") {
        // items internal anyOf should also be merged
        assert!(items.get("anyOf").is_none());
        assert!(items.get("propertyNames").is_none());
        assert!(items.get("const").is_none());
    }
}

#[test]
fn test_fix_tool_call_args() {
    let mut args = serde_json::json!({
        "port": "8080",
        "enabled": "true",
        "timeout": "5.5",
        "metadata": {
            "retry": "3"
        },
        "tags": ["1", "2"]
    });

    let schema = serde_json::json!({
        "properties": {
            "port": { "type": "integer" },
            "enabled": { "type": "boolean" },
            "timeout": { "type": "number" },
            "metadata": {
                "type": "object",
                "properties": {
                    "retry": { "type": "integer" }
                }
            },
            "tags": {
                "type": "array",
                "items": { "type": "integer" }
            }
        }
    });

    fix_tool_call_args(&mut args, &schema);

    assert_eq!(args["port"], 8080);
    assert_eq!(args["enabled"], true);
    assert_eq!(args["timeout"], 5.5);
    assert_eq!(args["metadata"]["retry"], 3);
    assert_eq!(args["tags"], serde_json::json!([1, 2]));
}

#[test]
fn test_fix_tool_call_args_protection() {
    let mut args = serde_json::json!({
        "version": "01.0",
        "code": "007"
    });

    let schema = serde_json::json!({
        "properties": {
            "version": { "type": "number" },
            "code": { "type": "integer" }
        }
    });

    fix_tool_call_args(&mut args, &schema);

    // Should preserve strings to prevent semantic breakage
    assert_eq!(args["version"], "01.0");
    assert_eq!(args["code"], "007");
}

#[test]
fn test_nested_defs_flattening() {
    // MCP tools often nest $defs inside properties, not at root
    let mut schema = json!({
        "type": "object",
        "properties": {
            "config": {
                "$defs": {
                    "Address": {
                        "type": "object",
                        "properties": {
                            "city": { "type": "string" },
                            "zip": { "type": "string" }
                        }
                    }
                },
                "type": "object",
                "properties": {
                    "home": { "$ref": "#/$defs/Address" },
                    "work": { "$ref": "#/$defs/Address" }
                }
            }
        }
    });

    clean_json_schema(&mut schema);

    // Verify nested $ref is correctly resolved
    let home = &schema["properties"]["config"]["properties"]["home"];
    assert_eq!(
        home["type"], "object",
        "home should have type 'object' from resolved $ref"
    );
    assert_eq!(
        home["properties"]["city"]["type"], "string",
        "home.properties.city should exist from resolved Address"
    );

    // Verify no orphan $ref remains
    assert!(
        home.get("$ref").is_none(),
        "home should not have orphan $ref"
    );

    // Verify work is also correctly resolved
    let work = &schema["properties"]["config"]["properties"]["work"];
    assert_eq!(work["type"], "object");
    assert!(work.get("$ref").is_none());
}

#[test]
fn test_unresolved_ref_fallback() {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "external": { "$ref": "https://example.com/schemas/External.json" },
            "missing": { "$ref": "#/$defs/NonExistent" }
        }
    });

    clean_json_schema(&mut schema);

    // Verify external reference is downgraded to string type
    let external = &schema["properties"]["external"];
    assert_eq!(
        external["type"], "string",
        "unresolved external $ref should fallback to string"
    );
    assert!(
        external["description"]
            .as_str()
            .unwrap()
            .contains("Unresolved $ref"),
        "description should contain unresolved $ref hint"
    );

    // Verify internal missing reference is also downgraded
    let missing = &schema["properties"]["missing"];
    assert_eq!(missing["type"], "string");
    assert!(missing["description"]
        .as_str()
        .unwrap()
        .contains("NonExistent"));
}

#[test]
fn test_deeply_nested_multi_level_defs() {
    let mut schema = json!({
        "type": "object",
        "$defs": {
            "RootDef": { "type": "integer" }
        },
        "properties": {
            "level1": {
                "type": "object",
                "$defs": {
                    "Level1Def": { "type": "boolean" }
                },
                "properties": {
                    "level2": {
                        "type": "object",
                        "$defs": {
                            "Level2Def": { "type": "number" }
                        },
                        "properties": {
                            "useRoot": { "$ref": "#/$defs/RootDef" },
                            "useLevel1": { "$ref": "#/$defs/Level1Def" },
                            "useLevel2": { "$ref": "#/$defs/Level2Def" }
                        }
                    }
                }
            }
        }
    });

    clean_json_schema(&mut schema);

    let level2_props = &schema["properties"]["level1"]["properties"]["level2"]["properties"];

    // Verify all level $defs are correctly resolved
    assert_eq!(
        level2_props["useRoot"]["type"], "integer",
        "RootDef should resolve"
    );
    assert_eq!(
        level2_props["useLevel1"]["type"], "boolean",
        "Level1Def should resolve"
    );
    assert_eq!(
        level2_props["useLevel2"]["type"], "number",
        "Level2Def should resolve"
    );

    // Verify no orphan $ref remains
    assert!(level2_props["useRoot"].get("$ref").is_none());
    assert!(level2_props["useLevel1"].get("$ref").is_none());
    assert!(level2_props["useLevel2"].get("$ref").is_none());
}

#[test]
fn test_non_standard_field_cleaning_and_healing() {
    let mut schema = json!({
        "type": "array",
        "items": {
            "cornerRadius": { "type": "number" },
            "fillColor": { "type": "string" }
        }
    });

    clean_json_schema(&mut schema);

    // Verify non-standard fields in items are moved to properties with type: object
    let items = &schema["items"];
    assert_eq!(items["type"], "object", "Malformed items should be healed to type object");
    assert!(items.get("properties").is_some(), "Malformed items should have properties object");
    assert_eq!(items["properties"]["cornerRadius"]["type"], "number");
    assert_eq!(items["properties"]["fillColor"]["type"], "string");

    // Verify original fields are removed from items top level (whitelist filter)
    assert!(items.get("cornerRadius").is_none());
    assert!(items.get("fillColor").is_none());
}

#[test]
fn test_implicit_type_injection() {
    let mut schema = json!({
        "properties": {
            "values": {
                "items": {
                    "cornerRadius": { "type": "number" }
                }
            }
        }
    });

    clean_json_schema(&mut schema);

    // Verify values is injected with type: array
    assert_eq!(schema["properties"]["values"]["type"], "array");

    // Verify items is heuristically fixed to type: object with properties
    let items = &schema["properties"]["values"]["items"];
    assert_eq!(items["type"], "object");
    assert!(items["properties"].get("cornerRadius").is_some());
}

#[test]
fn test_gemini_strict_validation_injection() {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "patterns": {
                "items": {
                    "properties": {
                        "type": {
                            "enum": ["A", "B"]
                        }
                    }
                }
            },
            "nested_props": {
                "properties": {
                    "foo": { "type": "string" }
                }
            }
        }
    });

    clean_json_schema(&mut schema);

    // Verify enum auto-completes type: string
    let type_node = &schema["properties"]["patterns"]["items"]["properties"]["type"];
    assert_eq!(type_node["type"], "string");
    assert!(type_node.get("enum").is_some());

    // Verify nested properties auto-completes type: object
    assert_eq!(schema["properties"]["nested_props"]["type"], "object");

    // Verify patterns auto-completes type: array
    assert_eq!(schema["properties"]["patterns"]["type"], "array");
}

#[test]
fn test_malformed_items_as_properties() {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "config": {
                "type": "object",
                "items": {
                    "color": { "type": "string" },
                    "size": { "type": "number" }
                }
            }
        }
    });

    clean_json_schema(&mut schema);

    // Verify items is removed and converted to properties
    let config = &schema["properties"]["config"];
    assert!(config.get("items").is_none());
    assert_eq!(config["properties"]["color"]["type"], "string");
    assert_eq!(config["properties"]["size"]["type"], "number");
    assert_eq!(config["type"], "object");
}
