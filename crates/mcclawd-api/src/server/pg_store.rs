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

    /// Insert a new task (upsert) with user_id, tags, and optional container tracking.
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

    /// Update the container_id and execution_mode for a task.
    pub async fn update_container_info(
        &self,
        id: &str,
        container_id: &str,
        execution_mode: &str,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "UPDATE tasks SET container_id = $2, execution_mode = $3, updated_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .bind(container_id)
        .bind(execution_mode)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Get container tracking info (container_id, execution_mode) for a task.
    pub async fn get_container_info(
        &self,
        id: &str,
    ) -> Result<Option<(String, String)>, McclawdError> {
        let row = sqlx::query_as::<_, (Option<String>, String)>(
            "SELECT container_id, execution_mode FROM tasks WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(row.map(|(cid, mode)| (cid.unwrap_or_default(), mode)))
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

    /// Persist the resolved tool configuration for a task.
    /// Called after skill resolution + tool filtering so the DB has a full audit trail
    /// and can recover the tool set on container restart/retry.
    pub async fn update_task_tools(
        &self,
        id: &str,
        selected_skills: &[String],
        allowed_tools: &[String],
        tool_profile: Option<&str>,
        skill_context: &str,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "UPDATE tasks SET selected_skills = $2, allowed_tools = $3, tool_profile = $4, skill_context = $5, updated_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .bind(selected_skills)
        .bind(allowed_tools)
        .bind(tool_profile)
        .bind(skill_context)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Read persisted tool configuration for a task (for container restart/retry).
    /// Returns (selected_skills, allowed_tools, tool_profile, skill_context) or None if task not found.
    pub async fn get_task_tools(
        &self,
        id: &str,
    ) -> Result<Option<(Vec<String>, Vec<String>, Option<String>, String)>, McclawdError> {
        let row = sqlx::query_as::<_, (Vec<String>, Vec<String>, Option<String>, String)>(
            "SELECT COALESCE(selected_skills, '{}'), COALESCE(allowed_tools, '{}'), tool_profile, COALESCE(skill_context, '') FROM tasks WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(row)
    }

    /// Delete a task and ALL related entities in a single transaction.
    /// Cascade order: dlp_findings (via FK CASCADE) → security_events → persistent_containers → task row.
    /// This is the single source of truth for task deletion — all callers should use this.
    pub async fn delete_task(&self, id: &str) -> Result<(), McclawdError> {
        let mut tx = self.pool.begin().await.map_err(pg_err)?;

        // 1. Delete security_events (dlp_findings auto-cascade via FK ON DELETE CASCADE)
        sqlx::query("DELETE FROM security_events WHERE task_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;

        // 2. Delete persistent container records for this task
        sqlx::query("DELETE FROM persistent_containers WHERE task_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;

        // 3. Delete the task row itself (events + chat_history cascade via FK)
        sqlx::query("DELETE FROM tasks WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;

        tx.commit().await.map_err(pg_err)?;
        Ok(())
    }

    /// Check if a task was last updated more than `hours` ago.
    /// Used by GC to apply retention period before deleting completed tasks.
    pub async fn is_task_older_than(&self, id: &str, hours: i64) -> Result<bool, McclawdError> {
        let row: Option<(bool,)> = sqlx::query_as(
            "SELECT updated_at < NOW() - make_interval(hours => $2) FROM tasks WHERE id = $1",
        )
        .bind(id)
        .bind(hours as i32)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(row.map(|(old,)| old).unwrap_or(true))
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

    /// Get a single task by ID (for lazy hydration on cache miss).
    /// Returns (id, prompt, status, error_message, tags, selected_skills, allowed_tools, tool_profile, skill_context) or None.
    pub async fn get_task(
        &self,
        id: &str,
    ) -> Result<Option<(String, String, String, Option<String>, Vec<String>, Vec<String>, Vec<String>, Option<String>, String)>, McclawdError> {
        let row = sqlx::query_as::<_, (String, String, String, Option<String>, Vec<String>, Vec<String>, Vec<String>, Option<String>, String)>(
            "SELECT id, prompt, status, error_message, COALESCE(tags, '{}'), COALESCE(selected_skills, '{}'), COALESCE(allowed_tools, '{}'), tool_profile, COALESCE(skill_context, '') FROM tasks WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(row)
    }

    /// Load all tasks from the database (for startup hydration).
    /// Returns (id, prompt, status, error_message, tags, selected_skills, allowed_tools, tool_profile, skill_context).
    pub async fn list_tasks(
        &self,
    ) -> Result<Vec<(String, String, String, Option<String>, Vec<String>, Vec<String>, Vec<String>, Option<String>, String)>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>, Vec<String>, Vec<String>, Vec<String>, Option<String>, String)>(
            "SELECT id, prompt, status, error_message, COALESCE(tags, '{}'), COALESCE(selected_skills, '{}'), COALESCE(allowed_tools, '{}'), tool_profile, COALESCE(skill_context, '') FROM tasks ORDER BY created_at ASC",
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
    ) -> Result<Vec<(String, String, String, Option<String>, Vec<String>, Vec<String>, Vec<String>, Option<String>, String)>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>, Vec<String>, Vec<String>, Vec<String>, Option<String>, String)>(
            "SELECT id, prompt, status, error_message, COALESCE(tags, '{}'), COALESCE(selected_skills, '{}'), COALESCE(allowed_tools, '{}'), tool_profile, COALESCE(skill_context, '') FROM tasks WHERE user_id = $1 AND $2 = ANY(tags) ORDER BY created_at ASC",
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

    /// Get a single config value for a user by key.
    pub async fn get_config_key(
        &self,
        user_id: &str,
        key: &str,
    ) -> Result<Option<serde_json::Value>, McclawdError> {
        let row = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT value FROM app_config WHERE user_id = $1 AND key = $2",
        )
        .bind(user_id)
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(row.map(|(v,)| v))
    }

    /// Delete a single config key for a user.
    pub async fn delete_config_key(
        &self,
        user_id: &str,
        key: &str,
    ) -> Result<(), McclawdError> {
        sqlx::query("DELETE FROM app_config WHERE user_id = $1 AND key = $2")
            .bind(user_id)
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    /// List all config keys and their string-serialized values for a user.
    pub async fn list_configs(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, String)>, McclawdError> {
        let rows = self.load_config(user_id).await?;
        Ok(rows
            .into_iter()
            .map(|(k, v)| {
                let s = if let serde_json::Value::String(s) = &v {
                    s.clone()
                } else {
                    v.to_string()
                };
                (k, s)
            })
            .collect())
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

    /// Get a single workspace file for a user.
    pub async fn get_workspace_file(
        &self,
        user_id: &str,
        workspace: &str,
        filename: &str,
    ) -> Result<Option<String>, McclawdError> {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT content FROM workspace_files WHERE user_id = $1 AND workspace = $2 AND filename = $3",
        )
        .bind(user_id)
        .bind(workspace)
        .bind(filename)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(row.map(|r| r.0))
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
    // Workspace profiles (tenant-scoped)
    // -----------------------------------------------------------------------

    /// List custom workspace profiles for a user (name + description).
    pub async fn list_workspace_profiles(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, String)>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT DISTINCT profile_name, COALESCE(MAX(description), '') FROM workspace_profiles WHERE user_id = $1 GROUP BY profile_name ORDER BY profile_name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(rows)
    }

    /// Save a workspace profile (set of files) for a user.
    pub async fn save_workspace_profile(
        &self,
        user_id: &str,
        name: &str,
        description: &str,
        files: &[(String, String)],
    ) -> Result<(), McclawdError> {
        // Delete existing profile files
        sqlx::query("DELETE FROM workspace_profiles WHERE user_id = $1 AND profile_name = $2")
            .bind(user_id)
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;

        // Insert new files
        for (filename, content) in files {
            sqlx::query(
                "INSERT INTO workspace_profiles (user_id, profile_name, description, filename, content) VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(user_id)
            .bind(name)
            .bind(description)
            .bind(filename)
            .bind(content)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        }
        Ok(())
    }

    /// Load a workspace profile by name.
    pub async fn load_workspace_profile(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<Option<Vec<(String, String)>>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT filename, content FROM workspace_profiles WHERE user_id = $1 AND profile_name = $2",
        )
        .bind(user_id)
        .bind(name)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        if rows.is_empty() {
            Ok(None)
        } else {
            Ok(Some(rows))
        }
    }

    /// Delete a custom workspace profile.
    pub async fn delete_workspace_profile(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<bool, McclawdError> {
        let result =
            sqlx::query("DELETE FROM workspace_profiles WHERE user_id = $1 AND profile_name = $2")
                .bind(user_id)
                .bind(name)
                .execute(&self.pool)
                .await
                .map_err(pg_err)?;
        Ok(result.rows_affected() > 0)
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
    // -----------------------------------------------------------------------
    // Persistent containers (survive server restarts)
    // -----------------------------------------------------------------------

    /// Record a persistent container so we can reconnect on restart.
    pub async fn save_persistent_container(
        &self,
        container_id: &str,
        task_id: &str,
        agent_type: &str,
        workspace_dir: &str,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "INSERT INTO persistent_containers (container_id, task_id, agent_type, workspace_dir)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (container_id) DO UPDATE SET last_seen_at = NOW()",
        )
        .bind(container_id)
        .bind(task_id)
        .bind(agent_type)
        .bind(workspace_dir)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Remove a persistent container record (on cleanup/delete).
    pub async fn delete_persistent_container(
        &self,
        container_id: &str,
    ) -> Result<(), McclawdError> {
        sqlx::query("DELETE FROM persistent_containers WHERE container_id = $1")
            .bind(container_id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    /// Remove all persistent container records for a task.
    pub async fn delete_persistent_containers_by_task(
        &self,
        task_id: &str,
    ) -> Result<(), McclawdError> {
        sqlx::query("DELETE FROM persistent_containers WHERE task_id = $1")
            .bind(task_id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    /// Look up container IDs associated with a task (for cleanup when no in-memory handle exists).
    pub async fn get_container_ids_by_task(
        &self,
        task_id: &str,
    ) -> Result<Vec<String>, McclawdError> {
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT container_id FROM persistent_containers WHERE task_id = $1",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(rows.into_iter().map(|(cid,)| cid).collect())
    }

    /// Remove all security events (and cascaded dlp_findings) for a task.
    /// dlp_findings rows are auto-deleted via ON DELETE CASCADE on security_event_id FK.
    pub async fn delete_security_events_by_task(
        &self,
        task_id: &str,
    ) -> Result<(), McclawdError> {
        sqlx::query("DELETE FROM security_events WHERE task_id = $1")
            .bind(task_id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    /// Load all persistent container records (for startup reconnection).
    /// Returns (container_id, task_id, agent_type, workspace_dir).
    pub async fn load_persistent_containers(
        &self,
    ) -> Result<Vec<(String, String, String, String)>, McclawdError> {
        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT container_id, task_id, agent_type, workspace_dir FROM persistent_containers ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(rows)
    }

    /// Update last_seen_at timestamp (heartbeat).
    pub async fn touch_persistent_container(
        &self,
        container_id: &str,
    ) -> Result<(), McclawdError> {
        sqlx::query(
            "UPDATE persistent_containers SET last_seen_at = NOW() WHERE container_id = $1",
        )
        .bind(container_id)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    // ─── Security Events ────────────────────────────────────────────

    pub async fn insert_security_event(
        &self,
        task_id: Option<&str>,
        user_id: &str,
        agent_id: Option<&str>,
        trace_id: Option<&str>,
        span_id: Option<&str>,
        event_type: &str,
        tool_name: Option<&str>,
        direction: Option<&str>,
        threat_level: Option<&str>,
        details: &serde_json::Value,
        action_taken: &str,
    ) -> anyhow::Result<i64> {
        let row = sqlx::query_scalar::<_, i64>(
            "INSERT INTO security_events (task_id, user_id, agent_id, trace_id, span_id, event_type, tool_name, direction, threat_level, details, action_taken)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             RETURNING id"
        )
        .bind(task_id)
        .bind(user_id)
        .bind(agent_id)
        .bind(trace_id)
        .bind(span_id)
        .bind(event_type)
        .bind(tool_name)
        .bind(direction)
        .bind(threat_level)
        .bind(details)
        .bind(action_taken)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn insert_dlp_finding(
        &self,
        security_event_id: i64,
        finding_type: &str,
        tag: &str,
        pattern_name: Option<&str>,
        confidence: Option<f32>,
        data_hash: Option<&str>,
        redacted_preview: Option<&str>,
        source_text: Option<&str>,
        match_offset: Option<i32>,
        match_length: Option<i32>,
    ) -> anyhow::Result<i64> {
        let row = sqlx::query_scalar::<_, i64>(
            "INSERT INTO dlp_findings (security_event_id, finding_type, tag, pattern_name, confidence, data_hash, redacted_preview, source_text, match_offset, match_length)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             RETURNING id",
        )
        .bind(security_event_id)
        .bind(finding_type)
        .bind(tag)
        .bind(pattern_name)
        .bind(confidence)
        .bind(data_hash)
        .bind(redacted_preview)
        .bind(source_text)
        .bind(match_offset)
        .bind(match_length)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list_security_events(
        &self,
        task_id: Option<&str>,
        since: Option<chrono::DateTime<chrono::Utc>>,
        limit: i64,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT json_build_object(
                'id', e.id, 'task_id', e.task_id, 'agent_id', e.agent_id,
                'trace_id', e.trace_id, 'span_id', e.span_id,
                'event_type', e.event_type, 'tool_name', e.tool_name,
                'direction', e.direction, 'threat_level', e.threat_level,
                'details', e.details, 'action_taken', e.action_taken,
                'created_at', e.created_at,
                'findings', COALESCE((
                    SELECT json_agg(json_build_object(
                        'id', f.id, 'finding_type', f.finding_type,
                        'tag', f.tag, 'pattern_name', f.pattern_name,
                        'confidence', f.confidence, 'redacted_preview', f.redacted_preview,
                        'source_text', f.source_text, 'match_offset', f.match_offset, 'match_length', f.match_length
                    ))
                    FROM dlp_findings f WHERE f.security_event_id = e.id
                ), '[]'::json)
            )
            FROM security_events e
            WHERE ($1::text IS NULL OR e.task_id = $1)
              AND ($2::timestamptz IS NULL OR e.created_at >= $2)
            ORDER BY e.created_at DESC
            LIMIT $3"
        )
        .bind(task_id)
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Events grouped by task, with task prompt and per-event findings.
    pub async fn list_events_grouped_by_task(
        &self,
        since: Option<chrono::DateTime<chrono::Utc>>,
        limit: i64,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT json_build_object(
                'task_id', e.task_id,
                'task_prompt', COALESCE(t.prompt, ''),
                'task_status', COALESCE(t.status, ''),
                'event_count', COUNT(*),
                'finding_count', COALESCE(SUM((SELECT COUNT(*) FROM dlp_findings f WHERE f.security_event_id = e.id)), 0),
                'threat_levels', COALESCE((
                    SELECT json_object_agg(tl, cnt) FROM (
                        SELECT COALESCE(e2.threat_level, 'none') as tl, COUNT(*) as cnt
                        FROM security_events e2 WHERE e2.task_id = e.task_id
                          AND ($1::timestamptz IS NULL OR e2.created_at >= $1)
                        GROUP BY e2.threat_level
                    ) sub
                ), '{}'::json),
                'events', COALESCE((
                    SELECT json_agg(ev ORDER BY ev->>'created_at' DESC) FROM (
                        SELECT json_build_object(
                            'id', e3.id, 'event_type', e3.event_type,
                            'tool_name', e3.tool_name, 'direction', e3.direction,
                            'threat_level', e3.threat_level, 'action_taken', e3.action_taken,
                            'details', e3.details, 'created_at', e3.created_at,
                            'findings', COALESCE((
                                SELECT json_agg(json_build_object(
                                    'id', f.id, 'finding_type', f.finding_type,
                                    'tag', f.tag, 'pattern_name', f.pattern_name,
                                    'confidence', f.confidence, 'redacted_preview', f.redacted_preview,
                                    'source_text', f.source_text, 'match_offset', f.match_offset, 'match_length', f.match_length
                                )) FROM dlp_findings f WHERE f.security_event_id = e3.id
                            ), '[]'::json)
                        ) as ev
                        FROM security_events e3 WHERE e3.task_id = e.task_id
                          AND ($1::timestamptz IS NULL OR e3.created_at >= $1)
                          AND e3.action_taken != 'allowed'
                          AND EXISTS (SELECT 1 FROM dlp_findings f2 WHERE f2.security_event_id = e3.id)
                        ORDER BY e3.created_at DESC
                        LIMIT 50
                    ) sub
                ), '[]'::json)
            )
            FROM security_events e
            LEFT JOIN tasks t ON t.id = e.task_id
            WHERE ($1::timestamptz IS NULL OR e.created_at >= $1)
              AND (t.id IS NOT NULL OR e.task_id = '__system__')
              AND e.action_taken != 'allowed'
              AND EXISTS (SELECT 1 FROM dlp_findings df JOIN security_events se2 ON se2.id = df.security_event_id WHERE se2.task_id = e.task_id)
            GROUP BY e.task_id, t.prompt, t.status
            ORDER BY MAX(e.created_at) DESC
            LIMIT $2"
        )
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn security_summary(
        &self,
        user_id: &str,
        since: Option<chrono::DateTime<chrono::Utc>>,
    ) -> anyhow::Result<serde_json::Value> {
        let row = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT json_build_object(
                'total_events', COUNT(*),
                'blocked', COUNT(*) FILTER (WHERE action_taken = 'blocked'),
                'warned', COUNT(*) FILTER (WHERE action_taken = 'warned'),
                'allowed', COUNT(*) FILTER (WHERE action_taken = 'allowed'),
                'by_type', COALESCE((
                    SELECT json_object_agg(event_type, cnt)
                    FROM (SELECT se.event_type, COUNT(*) as cnt FROM security_events se
                          LEFT JOIN tasks tt ON tt.id = se.task_id
                          WHERE se.user_id = $1 AND ($2::timestamptz IS NULL OR se.created_at >= $2)
                            AND (tt.id IS NOT NULL OR se.task_id = '__system__')
                            AND se.action_taken != 'allowed'
                            AND EXISTS (SELECT 1 FROM dlp_findings df WHERE df.security_event_id = se.id)
                          GROUP BY se.event_type) sub
                ), '{}'::json),
                'by_threat', COALESCE((
                    SELECT json_object_agg(threat_level, cnt)
                    FROM (SELECT COALESCE(se.threat_level, 'unknown') as threat_level, COUNT(*) as cnt
                          FROM security_events se
                          LEFT JOIN tasks tt ON tt.id = se.task_id
                          WHERE se.user_id = $1 AND ($2::timestamptz IS NULL OR se.created_at >= $2)
                            AND (tt.id IS NOT NULL OR se.task_id = '__system__')
                            AND se.action_taken != 'allowed'
                            AND EXISTS (SELECT 1 FROM dlp_findings df WHERE df.security_event_id = se.id)
                          GROUP BY se.threat_level) sub
                ), '{}'::json)
            )
            FROM security_events e
            LEFT JOIN tasks t ON t.id = e.task_id
            WHERE e.user_id = $1 AND ($2::timestamptz IS NULL OR e.created_at >= $2)
              AND (t.id IS NOT NULL OR e.task_id = '__system__')
              AND e.action_taken != 'allowed'
              AND EXISTS (SELECT 1 FROM dlp_findings df WHERE df.security_event_id = e.id)"
        )
        .bind(user_id)
        .bind(since)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Remove orphaned security data: events and findings for tasks that no longer exist.
    /// Safe to call at startup to clean up stale data from previously deleted tasks.
    pub async fn cleanup_orphaned_security_events(&self) -> anyhow::Result<u64> {
        // Delete findings for events whose tasks no longer exist
        sqlx::query("DELETE FROM dlp_findings WHERE security_event_id IN (SELECT se.id FROM security_events se LEFT JOIN tasks t ON t.id = se.task_id WHERE t.id IS NULL AND se.task_id != '__system__')")
            .execute(&self.pool)
            .await?;

        let result = sqlx::query("DELETE FROM security_events WHERE task_id NOT IN (SELECT id FROM tasks) AND task_id != '__system__'")
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    /// Remove security events that are noise: no findings OR action=allowed.
    /// Only warnings, blocks, and redactions with actual findings are kept.
    pub async fn cleanup_events_without_findings(&self) -> anyhow::Result<u64> {
        // Delete findings attached to allowed events (clean up FK before deleting events)
        sqlx::query("DELETE FROM dlp_findings WHERE security_event_id IN (SELECT id FROM security_events WHERE action_taken = 'allowed')")
            .execute(&self.pool)
            .await?;
        // Delete allowed events + events with no findings
        let result = sqlx::query(
            "DELETE FROM security_events WHERE action_taken = 'allowed' OR id NOT IN (SELECT DISTINCT security_event_id FROM dlp_findings)",
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn list_dlp_policies(&self) -> anyhow::Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT json_build_object(
                'id', id, 'name', name, 'description', description,
                'tag_pattern', tag_pattern, 'tool_pattern', tool_pattern,
                'action', action, 'enabled', enabled,
                'created_at', created_at, 'updated_at', updated_at
            )
            FROM dlp_policies ORDER BY id"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn upsert_dlp_policy(
        &self,
        name: &str,
        description: Option<&str>,
        tag_pattern: &str,
        tool_pattern: Option<&str>,
        action: &str,
        enabled: bool,
    ) -> anyhow::Result<i32> {
        let row = sqlx::query_scalar::<_, i32>(
            "INSERT INTO dlp_policies (name, description, tag_pattern, tool_pattern, action, enabled, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, NOW())
             ON CONFLICT (name) DO UPDATE SET
               description = EXCLUDED.description,
               tag_pattern = EXCLUDED.tag_pattern,
               tool_pattern = EXCLUDED.tool_pattern,
               action = EXCLUDED.action,
               enabled = EXCLUDED.enabled,
               updated_at = NOW()
             RETURNING id"
        )
        .bind(name)
        .bind(description)
        .bind(tag_pattern)
        .bind(tool_pattern)
        .bind(action)
        .bind(enabled)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn delete_dlp_policy(&self, id: i32) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM dlp_policies WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
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
