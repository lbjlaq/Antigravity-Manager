
    #[test]
    fn test_custom_web_search_function_downgrade() {
        // Scenario: User provides a custom tool named "web_search" via functionDeclarations
        // This is NOT a native Google Search request, but a custom tool.
        let tools = Some(vec![json!({
            "functionDeclarations": [
                { "name": "web_search", "parameters": {} } // Custom function
            ]
        })]);
        
        let config = resolve_request_config("gemini-1.5-pro", "gemini-1.5-pro", &tools);
        
        // Current logic expects:
        // 1. detects_networking_tool -> true (because name is "web_search", line 210)
        // 2. enable_networking -> true
        // 3. request_type -> "web_search"
        // 4. final_model -> downgraded to "gemini-2.5-flash"
        
        assert_eq!(config.request_type, "web_search");
        assert_eq!(config.final_model, "gemini-2.5-flash");
        assert!(config.inject_google_search); // It thinks it should inject, but inject_tool will skip it later
    }
