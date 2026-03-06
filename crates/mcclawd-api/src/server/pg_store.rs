//! PostgreSQL-backed persistence for tasks, events, chat history, usage data,
//! config, workspace files, skills, MCP servers, scan cache, schedules, and swarms.

use mcclawd_channels::OutboundChunk;
use mcclawd_core::config::McpServerConfig;
use mcclawd_core::providers::{DailyUsage, ModelUsageEntry, TaskUsageEntry};
use mcclawd_core::McclawdError;
use rig::completion::message::Message;
use sqlx::PgPool;

/// PostgreSQL task store — persists all tenant-scoped application data.
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

    /// Insert a new task (upsert) with user_id and tags.
    pub async fn save_task(
        &self,
        id: &str,
        prompt: &str,
        status: &str,
        error_message: Option<&str>,
        user_id: &str,
        tags: &[String],
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "INSERT INTO tasks (id, prompt, status, error_message, user_id, tags) VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE SET status = $3, error_message = $4, tags = $6, updated_at = NOW()",
        )
        .bind(id)
        .bind(prompt)
        .bind(status)
        .bind(error_message)
        .bind(user_id)
        .bind(tags)
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

    /// Update tags on a task.
    pub async fn update_task_tags(
        &self,
        id: &str,
        tags: &[String],
    ) -> Result<(), McclawdError> {
        sqlx::query("UPDATE tasks SET tags = $2, updated_at = NOW() WHERE id = $1")
            .bind(id)
            .bind(tags)
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

    /// Delete all tasks for a user, optionally filtered by tag.
    pub async fn delete_tasks_by_tag(
        &self,
        user_id: &str,
        tag: Option<&str>,
    ) -> Result<u64, McclawdError> {
        let result = if let Some(tag) = tag {
            sqlx::query("DELETE FROM tasks WHERE user_id = $1 AND $2 = ANY(tags)")
                .bind(user_id)
                .bind(tag)
                .execute(&self.pool)
                .await
                .map_err(pg_err)?
        } else {
            sqlx::query("DELETE FROM tasks WHERE user_id = $1")
                .bind(user_id)
                .execute(&self.pool)
                .await
                .map_err(pg_err)?
        };
        Ok(result.rows_affected())
    }

    /// Load all tasks from the database (for startup hydration).
    /// Returns (id, prompt, status, error_message, tags).
    pub async fn list_tasks(
        &self,
    ) -> Result<Vec<(String, String, String, Option<String>, Vec<String>)>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>, Vec<String>)>(
            "SELECT id, prompt, status, error_message, COALESCE(tags, '{}') FROM tasks ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(rows)
    }

    /// Load tasks filtered by tag for a user.
    pub async fn list_tasks_by_tag(
        &self,
        user_id: &str,
        tag: &str,
    ) -> Result<Vec<(String, String, String, Option<String>, Vec<String>)>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>, Vec<String>)>(
            "SELECT id, prompt, status, error_message, COALESCE(tags, '{}') FROM tasks WHERE user_id = $1 AND $2 = ANY(tags) ORDER BY created_at ASC",
        )
        .bind(user_id)
        .bind(tag)
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

    // -----------------------------------------------------------------------
    // Usage tracking (tenant-scoped)
    // -----------------------------------------------------------------------

    /// Upsert a daily usage record (accumulates cost and tokens for a given date).
    pub async fn upsert_daily_usage(
        &self,
        user_id: &str,
        date: &str,
        cost_usd: f64,
        tokens: u64,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "INSERT INTO daily_usage (user_id, date, cost_usd, tokens) VALUES ($1, $2::date, $3, $4)
             ON CONFLICT (user_id, date) DO UPDATE SET cost_usd = daily_usage.cost_usd + $3, tokens = daily_usage.tokens + $4",
        )
        .bind(user_id)
        .bind(date)
        .bind(cost_usd)
        .bind(tokens as i64)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Load all daily usage records, oldest first.
    pub async fn load_daily_usage(&self) -> Result<Vec<DailyUsage>, McclawdError> {
        let rows = sqlx::query_as::<_, (chrono::NaiveDate, f64, i64)>(
            "SELECT date, cost_usd, tokens FROM daily_usage ORDER BY date ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(rows
            .into_iter()
            .map(|(date, cost_usd, tokens)| DailyUsage {
                date: date.format("%Y-%m-%d").to_string(),
                cost_usd,
                tokens: tokens as u64,
            })
            .collect())
    }

    /// Upsert a model usage record (accumulates tokens and cost).
    pub async fn upsert_model_usage(
        &self,
        user_id: &str,
        entry: &ModelUsageEntry,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "INSERT INTO model_usage (user_id, model, input_tokens, output_tokens, total_tokens, estimated_cost_usd, request_count)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (user_id, model) DO UPDATE SET
                input_tokens = model_usage.input_tokens + $3,
                output_tokens = model_usage.output_tokens + $4,
                total_tokens = model_usage.total_tokens + $5,
                estimated_cost_usd = model_usage.estimated_cost_usd + $6,
                request_count = model_usage.request_count + $7",
        )
        .bind(user_id)
        .bind(&entry.model)
        .bind(entry.input_tokens as i64)
        .bind(entry.output_tokens as i64)
        .bind(entry.total_tokens as i64)
        .bind(entry.estimated_cost_usd)
        .bind(entry.request_count as i64)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Load all model usage records.
    pub async fn load_model_usage(&self) -> Result<Vec<ModelUsageEntry>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, i64, i64, i64, f64, i64)>(
            "SELECT model, input_tokens, output_tokens, total_tokens, estimated_cost_usd, request_count FROM model_usage ORDER BY estimated_cost_usd DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(rows
            .into_iter()
            .map(|(model, input, output, total, cost, count)| ModelUsageEntry {
                model,
                input_tokens: input as u64,
                output_tokens: output as u64,
                total_tokens: total as u64,
                estimated_cost_usd: cost,
                request_count: count as u64,
            })
            .collect())
    }

    /// Insert a task usage record.
    pub async fn insert_task_usage(
        &self,
        user_id: &str,
        entry: &TaskUsageEntry,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "INSERT INTO task_usage (user_id, task_id, prompt_preview, model, total_tokens, estimated_cost_usd) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(user_id)
        .bind(&entry.task_id)
        .bind(&entry.prompt_preview)
        .bind(&entry.model)
        .bind(entry.total_tokens as i64)
        .bind(entry.estimated_cost_usd)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Load recent task usage records (most recent first, limited to 50).
    pub async fn load_task_usage(&self) -> Result<Vec<TaskUsageEntry>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, String, String, i64, f64)>(
            "SELECT task_id, prompt_preview, model, total_tokens, estimated_cost_usd FROM task_usage ORDER BY created_at DESC LIMIT 50",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(rows
            .into_iter()
            .map(|(task_id, prompt_preview, model, tokens, cost)| TaskUsageEntry {
                task_id,
                prompt_preview,
                model,
                total_tokens: tokens as u64,
                estimated_cost_usd: cost,
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // App config (tenant-scoped key-value)
    // -----------------------------------------------------------------------

    /// Save a config key-value pair for a user.
    pub async fn save_config(
        &self,
        user_id: &str,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "INSERT INTO app_config (user_id, key, value, updated_at) VALUES ($1, $2, $3, NOW())
             ON CONFLICT (user_id, key) DO UPDATE SET value = $3, updated_at = NOW()",
        )
        .bind(user_id)
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Load all config key-value pairs for a user.
    pub async fn load_config(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, serde_json::Value)>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
            "SELECT key, value FROM app_config WHERE user_id = $1 ORDER BY key",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(rows)
    }

    // -----------------------------------------------------------------------
    // Workspace files (tenant-scoped)
    // -----------------------------------------------------------------------

    /// Save a workspace file for a user.
    pub async fn save_workspace_file(
        &self,
        user_id: &str,
        workspace: &str,
        filename: &str,
        content: &str,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "INSERT INTO workspace_files (user_id, workspace, filename, content, updated_at) VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (user_id, workspace, filename) DO UPDATE SET content = $4, updated_at = NOW()",
        )
        .bind(user_id)
        .bind(workspace)
        .bind(filename)
        .bind(content)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Load all workspace files for a user in a given workspace.
    pub async fn load_workspace_files(
        &self,
        user_id: &str,
        workspace: &str,
    ) -> Result<Vec<(String, String)>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT filename, content FROM workspace_files WHERE user_id = $1 AND workspace = $2 ORDER BY filename",
        )
        .bind(user_id)
        .bind(workspace)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(rows)
    }

    // -----------------------------------------------------------------------
    // Installed skills (tenant-scoped)
    // -----------------------------------------------------------------------

    /// Save an installed skill for a user.
    pub async fn save_skill(
        &self,
        user_id: &str,
        name: &str,
        version: Option<&str>,
        skill_md: Option<&str>,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "INSERT INTO installed_skills (user_id, name, version, skill_md, installed_at) VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (user_id, name) DO UPDATE SET version = $3, skill_md = $4, installed_at = NOW()",
        )
        .bind(user_id)
        .bind(name)
        .bind(version)
        .bind(skill_md)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Load all installed skills for a user.
    pub async fn load_skills(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, Option<String>, Option<String>)>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            "SELECT name, version, skill_md FROM installed_skills WHERE user_id = $1 ORDER BY name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(rows)
    }

    /// Delete an installed skill for a user.
    pub async fn delete_skill(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<(), McclawdError> {
        sqlx::query("DELETE FROM installed_skills WHERE user_id = $1 AND name = $2")
            .bind(user_id)
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // MCP server configurations (tenant-scoped)
    // -----------------------------------------------------------------------

    /// Save an MCP server config for a user.
    pub async fn save_mcp_server(
        &self,
        user_id: &str,
        name: &str,
        config: &serde_json::Value,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "INSERT INTO mcp_servers (user_id, name, config, updated_at) VALUES ($1, $2, $3, NOW())
             ON CONFLICT (user_id, name) DO UPDATE SET config = $3, updated_at = NOW()",
        )
        .bind(user_id)
        .bind(name)
        .bind(config)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Load all MCP server configs for a user.
    pub async fn load_mcp_servers(
        &self,
        user_id: &str,
    ) -> Result<Vec<McpServerConfig>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, serde_json::Value, bool)>(
            "SELECT name, config, enabled FROM mcp_servers WHERE user_id = $1 ORDER BY name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        let mut servers = Vec::with_capacity(rows.len());
        for (_name, config_json, _enabled) in rows {
            match serde_json::from_value::<McpServerConfig>(config_json) {
                Ok(cfg) => servers.push(cfg),
                Err(e) => tracing::warn!("Failed to deserialize MCP server config: {e}"),
            }
        }
        Ok(servers)
    }

    /// Delete an MCP server config for a user.
    pub async fn delete_mcp_server(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<(), McclawdError> {
        sqlx::query("DELETE FROM mcp_servers WHERE user_id = $1 AND name = $2")
            .bind(user_id)
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Scan cache (tenant-scoped)
    // -----------------------------------------------------------------------

    /// Save a scan result for a skill.
    pub async fn save_scan_result(
        &self,
        user_id: &str,
        skill_name: &str,
        result: &serde_json::Value,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "INSERT INTO scan_cache (user_id, skill_name, result, scanned_at) VALUES ($1, $2, $3, NOW())
             ON CONFLICT (user_id, skill_name) DO UPDATE SET result = $3, scanned_at = NOW()",
        )
        .bind(user_id)
        .bind(skill_name)
        .bind(result)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Load all scan results for a user.
    pub async fn load_scan_cache(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, serde_json::Value)>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
            "SELECT skill_name, result FROM scan_cache WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(rows)
    }

    // -----------------------------------------------------------------------
    // Scheduled tasks (tenant-scoped)
    // -----------------------------------------------------------------------

    /// Save a scheduled task.
    pub async fn save_scheduled_task(
        &self,
        user_id: &str,
        id: &str,
        name: &str,
        cron_expr: &str,
        prompt: &str,
        workspace: Option<&str>,
        enabled: bool,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "INSERT INTO scheduled_tasks (id, user_id, name, cron_expr, prompt, workspace, enabled)
             VALUES ($1::uuid, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (id) DO UPDATE SET name = $3, cron_expr = $4, prompt = $5, workspace = $6, enabled = $7",
        )
        .bind(id)
        .bind(user_id)
        .bind(name)
        .bind(cron_expr)
        .bind(prompt)
        .bind(workspace)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Update last_run_at and next_run_at for a scheduled task.
    pub async fn update_schedule_run(
        &self,
        id: &str,
        last_run: &str,
        next_run: Option<&str>,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "UPDATE scheduled_tasks SET last_run_at = $2::timestamptz, next_run_at = $3::timestamptz WHERE id = $1::uuid",
        )
        .bind(id)
        .bind(last_run)
        .bind(next_run)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Load all scheduled tasks for a user.
    pub async fn load_scheduled_tasks(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, String, String, String, Option<String>, bool)>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, bool)>(
            "SELECT id::text, name, cron_expr, prompt, workspace, enabled FROM scheduled_tasks WHERE user_id = $1 ORDER BY created_at ASC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(rows)
    }

    /// Delete a scheduled task.
    pub async fn delete_scheduled_task(
        &self,
        id: &str,
    ) -> Result<(), McclawdError> {
        sqlx::query("DELETE FROM scheduled_tasks WHERE id = $1::uuid")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Swarm runs (tenant-scoped)
    // -----------------------------------------------------------------------

    /// Save a swarm run.
    pub async fn save_swarm_run(
        &self,
        user_id: &str,
        id: &str,
        name: &str,
        status: &str,
        config: &serde_json::Value,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "INSERT INTO swarm_runs (id, user_id, name, status, config)
             VALUES ($1::uuid, $2, $3, $4, $5)
             ON CONFLICT (id) DO UPDATE SET status = $4, config = $5",
        )
        .bind(id)
        .bind(user_id)
        .bind(name)
        .bind(status)
        .bind(config)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Update swarm run status and optionally set result/completed_at.
    pub async fn update_swarm_run(
        &self,
        id: &str,
        status: &str,
        result: Option<&str>,
    ) -> Result<(), McclawdError> {
        let result_json = result.map(|r| serde_json::Value::String(r.to_string()));
        sqlx::query(
            "UPDATE swarm_runs SET status = $2, result = COALESCE($3, result),
             completed_at = CASE WHEN $2 IN ('completed', 'cancelled', 'failed') THEN NOW() ELSE completed_at END
             WHERE id = $1::uuid",
        )
        .bind(id)
        .bind(status)
        .bind(result_json)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Load all swarm runs for a user (most recent first).
    pub async fn load_swarm_runs(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, String, String, Option<serde_json::Value>)>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<serde_json::Value>)>(
            "SELECT id::text, name, status, result FROM swarm_runs WHERE user_id = $1 ORDER BY started_at DESC LIMIT 100",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(rows)
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
