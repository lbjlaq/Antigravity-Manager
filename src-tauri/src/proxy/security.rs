use crate::proxy::config::{ProxyAuthMode, ProxyConfig};
use crate::models::config::AppConfig; // Assuming AuthConfig might be here or just use ProxyConfig

#[derive(Debug, Clone)]
pub struct ProxySecurityConfig {
    pub auth_mode: ProxyAuthMode,
    pub api_key: String,
    pub allow_lan_access: bool,
}

impl ProxySecurityConfig {
    pub fn from_proxy_config(config: &ProxyConfig) -> Self {
        Self {
            auth_mode: config.auth_mode.clone(),
            api_key: config.api_key.clone(),
            allow_lan_access: config.allow_lan_access,
        }
    }

    pub fn effective_auth_mode(&self) -> ProxyAuthMode {
        match self.auth_mode {
            ProxyAuthMode::Auto => {
                if self.allow_lan_access {
                    ProxyAuthMode::AllExceptHealth
                } else {
                    ProxyAuthMode::Off
                }
            }
            ref other => other.clone(),
        }
    }
}

/// Thread-safe wrapper for security state
pub struct SecurityState {
    pub config: tokio::sync::RwLock<ProxySecurityConfig>,
}

impl SecurityState {
    pub fn new(_auth_config: Option<crate::models::config::AppConfig>, security_config: ProxySecurityConfig) -> Self {
        // Note: The previous code passed auth_config and security_config separately.
        // We will just use the security_config for now as it contains the relevant fields.
        Self {
            config: tokio::sync::RwLock::new(security_config),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_mode_resolves_off_for_local_only() {
        let s = ProxySecurityConfig {
            auth_mode: ProxyAuthMode::Auto,
            api_key: "sk-test".to_string(),
            allow_lan_access: false,
        };
        assert!(matches!(s.effective_auth_mode(), ProxyAuthMode::Off));
    }

    #[test]
    fn auto_mode_resolves_all_except_health_for_lan() {
        let s = ProxySecurityConfig {
            auth_mode: ProxyAuthMode::Auto,
            api_key: "sk-test".to_string(),
            allow_lan_access: true,
        };
        assert!(matches!(
            s.effective_auth_mode(),
            ProxyAuthMode::AllExceptHealth
        ));
    }
}
