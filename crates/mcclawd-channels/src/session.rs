use mcclawd_core::types::SessionId;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionKey {
    pub channel: String,
    pub peer_id: String,
}

pub struct SessionManager {
    sessions: HashMap<SessionKey, SessionId>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    pub fn get_or_create(&mut self, key: SessionKey) -> SessionId {
        self.sessions
            .entry(key)
            .or_insert_with(SessionId::new)
            .clone()
    }
}
