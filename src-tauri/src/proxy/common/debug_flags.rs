use once_cell::sync::Lazy;

fn parse_env_bool(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let v = v.trim();
            matches!(
                v,
                "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON" | "enable" | "ENABLE"
            )
        }
        Err(_) => false,
    }
}

static DEBUG_BODY_ENABLED: Lazy<bool> =
    Lazy::new(|| parse_env_bool("ANTIGRAVITY_PROXY_DEBUG_BODY"));

/// Enables logging of full request/response bodies and pretty-printed transformed payloads.
/// Default: disabled (for performance and sensitive data safety).
pub fn debug_body_enabled() -> bool {
    *DEBUG_BODY_ENABLED
}

