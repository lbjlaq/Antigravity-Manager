use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// 代理协议类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyProtocol {
    Http,
    Socks5,
}

impl fmt::Display for ProxyProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProxyProtocol::Http => write!(f, "http"),
            ProxyProtocol::Socks5 => write!(f, "socks5"),
        }
    }
}

/// 代理节点来源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxySource {
    System,
    AutoDetected,
    Custom,
}

/// 代理节点描述
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyNode {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub protocol: ProxyProtocol,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    pub source: ProxySource,
}

impl ProxyNode {
    pub fn to_url(&self) -> String {
        match (&self.username, &self.password) {
            (Some(u), Some(p)) if !u.is_empty() => {
                format!(
                    "{}://{}:{}@{}:{}",
                    self.protocol, u, p, self.host, self.port
                )
            }
            _ => format!("{}://{}:{}", self.protocol, self.host, self.port),
        }
    }

    pub fn parse_url(raw: &str, name: Option<String>) -> Result<Self, ProxyError> {
        let parsed = url::Url::parse(raw).map_err(|e| ProxyError::InvalidUrl(e.to_string()))?;
        let protocol = match parsed.scheme().to_ascii_lowercase().as_str() {
            "http" | "https" => ProxyProtocol::Http,
            "socks5" | "socks5h" | "socks" => ProxyProtocol::Socks5,
            other => {
                return Err(ProxyError::InvalidUrl(format!(
                    "Unsupported proxy scheme: {}",
                    other
                )))
            }
        };
        let host = parsed
            .host_str()
            .ok_or_else(|| ProxyError::InvalidUrl("Missing proxy host".into()))?
            .to_string();
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| ProxyError::InvalidUrl("Missing proxy port".into()))?;
        let username = if parsed.username().is_empty() {
            None
        } else {
            Some(parsed.username().to_string())
        };
        let password = parsed.password().map(|s| s.to_string());

        Ok(Self {
            name: name.unwrap_or_else(|| format!("Custom ({}:{})", host, port)),
            host,
            port,
            protocol,
            username,
            password,
            latency_ms: None,
            source: ProxySource::Custom,
        })
    }
}

/// 目标 Antigravity 应用类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetApp {
    /// Antigravity 经典桌面版 (/Applications/Antigravity.app)
    AntigravityApp,
    /// Antigravity IDE (/Applications/Antigravity IDE.app)
    AntigravityIde,
    /// Antigravity CLI (agy)
    AgyCli,
}

impl fmt::Display for TargetApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetApp::AntigravityApp => write!(f, "Antigravity"),
            TargetApp::AntigravityIde => write!(f, "Antigravity IDE"),
            TargetApp::AgyCli => write!(f, "agy CLI"),
        }
    }
}

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("Invalid proxy URL: {0}")]
    InvalidUrl(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Target application not found: {0}")]
    AppNotFound(String),
    #[error("Unsupported platform operation: {0}")]
    UnsupportedPlatform(String),
    #[error("Proxy protocol error: {0}")]
    Protocol(String),
}
