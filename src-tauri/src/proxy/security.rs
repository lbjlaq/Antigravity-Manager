use crate::proxy::config::{ProxyAuthMode, ProxyConfig};

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

