use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Default)]
pub struct ZaiVisionMcpState {
    sessions: Arc<Mutex<HashMap<String, ZaiVisionSession>>>,
}

#[derive(Debug, Clone)]
struct ZaiVisionSession {
    #[allow(dead_code)]
    created_at: std::time::Instant,
}

impl ZaiVisionMcpState {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create_session(&self) -> String {
        let session_id = uuid::Uuid::new_v4().to_string();
        let mut sessions = self.sessions.lock().await;
        sessions.insert(
            session_id.clone(),
            ZaiVisionSession {
                created_at: std::time::Instant::now(),
            },
        );
        session_id
    }

    pub async fn has_session(&self, session_id: &str) -> bool {
        let sessions = self.sessions.lock().await;
        sessions.contains_key(session_id)
    }

    pub async fn remove_session(&self, session_id: &str) {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(session_id);
    }
}
