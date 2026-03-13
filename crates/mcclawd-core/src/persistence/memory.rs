//! In-memory implementations of persistence traits.
//! Test-only — production code uses Postgres implementations
//! in `mcclawd-api::server::pg_store`.

#[cfg(test)]
mod inner {
    use async_trait::async_trait;
    use chrono::Utc;
    use dashmap::DashMap;

    use crate::persistence::agent_configs::{AgentConfig, AgentConfigStore};
    use crate::persistence::sessions::{Session, SessionStore, Turn, TurnRole};

    // -----------------------------------------------------------------------
    // InMemorySessionStore
    // -----------------------------------------------------------------------

    pub struct InMemorySessionStore {
        sessions: DashMap<String, Session>,
        turns: DashMap<String, Vec<Turn>>,
    }

    impl InMemorySessionStore {
        pub fn new() -> Self {
            Self {
                sessions: DashMap::new(),
                turns: DashMap::new(),
            }
        }
    }

    #[async_trait]
    impl SessionStore for InMemorySessionStore {
        async fn create_session(
            &self,
            channel_id: &str,
            peer_id: &str,
            platform: &str,
        ) -> crate::Result<Session> {
            let session = Session {
                id: uuid::Uuid::new_v4().to_string(),
                channel_id: channel_id.to_string(),
                peer_id: peer_id.to_string(),
                platform: platform.to_string(),
                started_at: Utc::now(),
                ended_at: None,
                metadata: serde_json::json!({}),
            };
            self.sessions.insert(session.id.clone(), session.clone());
            self.turns.insert(session.id.clone(), Vec::new());
            Ok(session)
        }

        async fn end_session(&self, session_id: &str) -> crate::Result<()> {
            let mut entry = self
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| crate::McclawdError::Persistence("Session not found".into()))?;
            entry.ended_at = Some(Utc::now());
            Ok(())
        }

        async fn get_session(&self, session_id: &str) -> crate::Result<Option<Session>> {
            Ok(self.sessions.get(session_id).map(|s| s.clone()))
        }

        async fn add_turn(
            &self,
            session_id: &str,
            role: TurnRole,
            content: &str,
            tool_calls: Option<serde_json::Value>,
        ) -> crate::Result<Turn> {
            if !self.sessions.contains_key(session_id) {
                return Err(crate::McclawdError::Persistence(
                    "Session not found".into(),
                ));
            }
            let turn = Turn {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: session_id.to_string(),
                role,
                content: content.to_string(),
                tool_calls,
                created_at: Utc::now(),
            };
            self.turns
                .entry(session_id.to_string())
                .or_default()
                .push(turn.clone());
            Ok(turn)
        }

        async fn get_turns(&self, session_id: &str) -> crate::Result<Vec<Turn>> {
            let mut turns = self
                .turns
                .get(session_id)
                .map(|t| t.clone())
                .unwrap_or_default();
            turns.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            Ok(turns)
        }

        async fn get_recent_sessions(
            &self,
            peer_id: &str,
            limit: usize,
        ) -> crate::Result<Vec<Session>> {
            let mut sessions: Vec<Session> = self
                .sessions
                .iter()
                .filter(|entry| entry.value().peer_id == peer_id)
                .map(|entry| entry.value().clone())
                .collect();
            sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
            sessions.truncate(limit);
            Ok(sessions)
        }
    }

    // -----------------------------------------------------------------------
    // InMemoryAgentConfigStore
    // -----------------------------------------------------------------------

    pub struct InMemoryAgentConfigStore {
        configs: DashMap<String, AgentConfig>,
    }

    impl InMemoryAgentConfigStore {
        pub fn new() -> Self {
            Self {
                configs: DashMap::new(),
            }
        }
    }

    #[async_trait]
    impl AgentConfigStore for InMemoryAgentConfigStore {
        async fn save_config(&self, config: &AgentConfig) -> crate::Result<()> {
            let mut to_save = config.clone();
            to_save.updated_at = Utc::now();
            self.configs.insert(config.name.clone(), to_save);
            Ok(())
        }

        async fn get_config(&self, name: &str) -> crate::Result<Option<AgentConfig>> {
            Ok(self.configs.get(name).map(|c| c.clone()))
        }

        async fn list_configs(&self) -> crate::Result<Vec<AgentConfig>> {
            Ok(self.configs.iter().map(|c| c.value().clone()).collect())
        }

        async fn delete_config(&self, name: &str) -> crate::Result<()> {
            self.configs.remove(name);
            Ok(())
        }
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn create_session_populates_fields() {
            let store = InMemorySessionStore::new();
            let session = store
                .create_session("chan-1", "peer-1", "cli")
                .await
                .unwrap();
            assert_eq!(session.channel_id, "chan-1");
            assert_eq!(session.peer_id, "peer-1");
            assert_eq!(session.platform, "cli");
            assert!(session.ended_at.is_none());
            assert!(!session.id.is_empty());
        }

        #[tokio::test]
        async fn end_session_sets_ended_at() {
            let store = InMemorySessionStore::new();
            let session = store
                .create_session("chan-1", "peer-1", "cli")
                .await
                .unwrap();
            store.end_session(&session.id).await.unwrap();
            let updated = store.get_session(&session.id).await.unwrap().unwrap();
            assert!(updated.ended_at.is_some());
        }

        #[tokio::test]
        async fn get_session_not_found_returns_none() {
            let store = InMemorySessionStore::new();
            let result = store.get_session("nonexistent").await.unwrap();
            assert!(result.is_none());
        }

        #[tokio::test]
        async fn add_turn_populates_fields() {
            let store = InMemorySessionStore::new();
            let session = store
                .create_session("chan-1", "peer-1", "cli")
                .await
                .unwrap();
            let turn = store
                .add_turn(&session.id, TurnRole::User, "hello", None)
                .await
                .unwrap();
            assert_eq!(turn.session_id, session.id);
            assert_eq!(turn.role, TurnRole::User);
            assert_eq!(turn.content, "hello");
            assert!(turn.tool_calls.is_none());
        }

        #[tokio::test]
        async fn add_turn_to_missing_session_fails() {
            let store = InMemorySessionStore::new();
            let result = store
                .add_turn("nonexistent", TurnRole::User, "hello", None)
                .await;
            assert!(result.is_err());
        }

        #[tokio::test]
        async fn get_turns_returns_ordered() {
            let store = InMemorySessionStore::new();
            let session = store
                .create_session("chan-1", "peer-1", "cli")
                .await
                .unwrap();
            store
                .add_turn(&session.id, TurnRole::User, "first", None)
                .await
                .unwrap();
            store
                .add_turn(&session.id, TurnRole::Assistant, "second", None)
                .await
                .unwrap();
            let turns = store.get_turns(&session.id).await.unwrap();
            assert_eq!(turns.len(), 2);
            assert_eq!(turns[0].content, "first");
            assert_eq!(turns[1].content, "second");
            assert!(turns[0].created_at <= turns[1].created_at);
        }

        #[tokio::test]
        async fn get_recent_sessions_filters_by_peer() {
            let store = InMemorySessionStore::new();
            store.create_session("chan-1", "peer-1", "cli").await.unwrap();
            store.create_session("chan-2", "peer-2", "cli").await.unwrap();
            store.create_session("chan-3", "peer-1", "web").await.unwrap();
            let sessions = store.get_recent_sessions("peer-1", 10).await.unwrap();
            assert_eq!(sessions.len(), 2);
            assert!(sessions.iter().all(|s| s.peer_id == "peer-1"));
        }

        #[tokio::test]
        async fn get_recent_sessions_respects_limit() {
            let store = InMemorySessionStore::new();
            for i in 0..5 {
                store
                    .create_session(&format!("chan-{i}"), "peer-1", "cli")
                    .await
                    .unwrap();
            }
            let sessions = store.get_recent_sessions("peer-1", 3).await.unwrap();
            assert_eq!(sessions.len(), 3);
        }

        #[tokio::test]
        async fn turn_with_tool_calls_json() {
            let store = InMemorySessionStore::new();
            let session = store
                .create_session("chan-1", "peer-1", "cli")
                .await
                .unwrap();
            let tc = serde_json::json!([{"name": "memory_store", "args": {"key": "foo"}}]);
            let turn = store
                .add_turn(&session.id, TurnRole::Tool, "result", Some(tc.clone()))
                .await
                .unwrap();
            assert_eq!(turn.tool_calls, Some(tc));
        }

        #[tokio::test]
        async fn save_and_get_config() {
            let store = InMemoryAgentConfigStore::new();
            let config = AgentConfig {
                id: uuid::Uuid::new_v4().to_string(),
                name: "default".into(),
                soul_md: Some("You are helpful.".into()),
                agents_md: None,
                user_md: None,
                model_config: serde_json::json!({"model": "claude-sonnet-4-20250514"}),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            store.save_config(&config).await.unwrap();
            let fetched = store.get_config("default").await.unwrap().unwrap();
            assert_eq!(fetched.name, "default");
            assert_eq!(fetched.soul_md, Some("You are helpful.".into()));
        }

        #[tokio::test]
        async fn list_configs_returns_all() {
            let store = InMemoryAgentConfigStore::new();
            for name in ["alpha", "beta", "gamma"] {
                let config = AgentConfig {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: name.into(),
                    soul_md: None,
                    agents_md: None,
                    user_md: None,
                    model_config: serde_json::json!({}),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                };
                store.save_config(&config).await.unwrap();
            }
            let all = store.list_configs().await.unwrap();
            assert_eq!(all.len(), 3);
        }

        #[tokio::test]
        async fn delete_config_removes_entry() {
            let store = InMemoryAgentConfigStore::new();
            let config = AgentConfig {
                id: uuid::Uuid::new_v4().to_string(),
                name: "to-delete".into(),
                soul_md: None,
                agents_md: None,
                user_md: None,
                model_config: serde_json::json!({}),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            store.save_config(&config).await.unwrap();
            store.delete_config("to-delete").await.unwrap();
            assert!(store.get_config("to-delete").await.unwrap().is_none());
        }

        #[tokio::test]
        async fn upsert_updates_existing_config() {
            let store = InMemoryAgentConfigStore::new();
            let config = AgentConfig {
                id: uuid::Uuid::new_v4().to_string(),
                name: "evolving".into(),
                soul_md: Some("v1".into()),
                agents_md: None,
                user_md: None,
                model_config: serde_json::json!({}),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            store.save_config(&config).await.unwrap();

            let mut updated = config.clone();
            updated.soul_md = Some("v2".into());
            store.save_config(&updated).await.unwrap();

            let fetched = store.get_config("evolving").await.unwrap().unwrap();
            assert_eq!(fetched.soul_md, Some("v2".into()));
        }

        #[tokio::test]
        async fn get_missing_config_returns_none() {
            let store = InMemoryAgentConfigStore::new();
            assert!(store.get_config("nope").await.unwrap().is_none());
        }
    }
}
