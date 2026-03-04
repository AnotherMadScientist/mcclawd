//! PostgreSQL-backed persistence for tasks, events, and chat history.

use mcclawd_channels::OutboundChunk;
use mcclawd_core::McclawdError;
use rig::completion::message::Message;
use sqlx::PgPool;

/// PostgreSQL task store — persists tasks, streaming events, and LLM conversation history.
#[derive(Clone)]
pub struct PgTaskStore {
    pool: PgPool,
}

fn pg_err(e: impl std::fmt::Display) -> McclawdError {
    McclawdError::Persistence(e.to_string())
}

impl PgTaskStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // -----------------------------------------------------------------------
    // Tasks
    // -----------------------------------------------------------------------

    /// Insert a new task (upsert).
    pub async fn save_task(
        &self,
        id: &str,
        prompt: &str,
        status: &str,
        error_message: Option<&str>,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "INSERT INTO tasks (id, prompt, status, error_message) VALUES ($1, $2, $3, $4)
             ON CONFLICT (id) DO UPDATE SET status = $3, error_message = $4, updated_at = NOW()",
        )
        .bind(id)
        .bind(prompt)
        .bind(status)
        .bind(error_message)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Update task status (and optional error message).
    pub async fn update_status(
        &self,
        id: &str,
        status: &str,
        error_message: Option<&str>,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "UPDATE tasks SET status = $2, error_message = $3, updated_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .bind(status)
        .bind(error_message)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Delete a task (cascades to events + chat history via FK).
    pub async fn delete_task(&self, id: &str) -> Result<(), McclawdError> {
        sqlx::query("DELETE FROM tasks WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    /// Load all tasks from the database (for startup hydration).
    pub async fn list_tasks(
        &self,
    ) -> Result<Vec<(String, String, String, Option<String>)>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>)>(
            "SELECT id, prompt, status, error_message FROM tasks ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(rows)
    }

    // -----------------------------------------------------------------------
    // Events
    // -----------------------------------------------------------------------

    /// Append a single event chunk for a task.
    pub async fn append_event(
        &self,
        task_id: &str,
        chunk: &OutboundChunk,
    ) -> Result<(), McclawdError> {
        let json = serde_json::to_value(chunk).map_err(pg_err)?;
        sqlx::query("INSERT INTO task_events (task_id, chunk) VALUES ($1, $2)")
            .bind(task_id)
            .bind(json)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    /// Get all persisted events for a task, ordered by insertion order.
    pub async fn get_events(&self, task_id: &str) -> Result<Vec<OutboundChunk>, McclawdError> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT chunk FROM task_events WHERE task_id = $1 ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        let mut chunks = Vec::with_capacity(rows.len());
        for (json,) in rows {
            let chunk: OutboundChunk = serde_json::from_value(json).map_err(pg_err)?;
            chunks.push(chunk);
        }
        Ok(chunks)
    }

    // -----------------------------------------------------------------------
    // Chat history
    // -----------------------------------------------------------------------

    /// Replace the entire chat history for a task (delete + insert).
    pub async fn set_chat_history(
        &self,
        task_id: &str,
        messages: &[Message],
    ) -> Result<(), McclawdError> {
        let mut tx = self.pool.begin().await.map_err(pg_err)?;

        sqlx::query("DELETE FROM task_chat_history WHERE task_id = $1")
            .bind(task_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;

        for (seq, msg) in messages.iter().enumerate() {
            let role = match msg {
                Message::User { .. } => "user",
                Message::Assistant { .. } => "assistant",
            };
            let content = serde_json::to_value(msg).map_err(pg_err)?;

            sqlx::query(
                "INSERT INTO task_chat_history (task_id, role, content, seq) VALUES ($1, $2, $3, $4)",
            )
            .bind(task_id)
            .bind(role)
            .bind(content)
            .bind(seq as i32)
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;
        }

        tx.commit().await.map_err(pg_err)?;
        Ok(())
    }

    /// Get the chat history for a task, ordered by sequence number.
    pub async fn get_chat_history(&self, task_id: &str) -> Result<Vec<Message>, McclawdError> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT content FROM task_chat_history WHERE task_id = $1 ORDER BY seq ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        let mut messages = Vec::with_capacity(rows.len());
        for (json,) in rows {
            let msg: Message = serde_json::from_value(json).map_err(pg_err)?;
            messages.push(msg);
        }
        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbound_chunk_to_json() {
        let chunk = OutboundChunk::TextBlock("hello".to_string());
        let json = serde_json::to_value(&chunk).unwrap();
        assert!(json.is_object());
    }

    #[test]
    fn rig_message_to_json() {
        let msg = Message::user("hello world");
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "user");
    }
}
