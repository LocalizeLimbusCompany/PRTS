//! Encrypted personal and project AI endpoint settings.

use sqlx::{PgConnection, PgPool};

use crate::models::{ProjectAiSetting, UserAiSetting};

pub async fn find_user(pool: &PgPool, user_id: i64) -> Result<Option<UserAiSetting>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM user_ai_settings WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
}

pub async fn find_user_for_update_tx(
    conn: &mut PgConnection,
    user_id: i64,
) -> Result<Option<UserAiSetting>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM user_ai_settings WHERE user_id = $1 FOR UPDATE")
        .bind(user_id)
        .fetch_optional(conn)
        .await
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_user_tx(
    conn: &mut PgConnection,
    user_id: i64,
    base_url: &str,
    model: &str,
    ciphertext: &[u8],
    nonce: &[u8],
    hint: &str,
    enabled: bool,
    provider_preset: &str,
    transport_mode: &str,
    thinking_mode: &str,
    reasoning_effort: &str,
    thinking_budget: Option<i64>,
    request_timeout_seconds: i32,
    custom_request_options: &serde_json::Value,
    web_search_mode: &str,
    web_search_provider: &str,
    web_search_endpoint: Option<&str>,
    web_search_api_key_ciphertext: Option<&[u8]>,
    web_search_api_key_nonce: Option<&[u8]>,
    web_search_api_key_hint: Option<&str>,
    web_search_timeout_seconds: i32,
    web_search_max_results: i32,
    web_search_citations_enabled: bool,
) -> Result<UserAiSetting, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO user_ai_settings (
             user_id, base_url, model, api_key_ciphertext, api_key_nonce, api_key_hint, enabled,
             provider_preset, transport_mode, thinking_mode, reasoning_effort, thinking_budget,
             request_timeout_seconds, custom_request_options, web_search_mode, web_search_provider,
             web_search_endpoint, web_search_api_key_ciphertext, web_search_api_key_nonce,
             web_search_api_key_hint, web_search_timeout_seconds, web_search_max_results,
             web_search_citations_enabled
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)
         ON CONFLICT (user_id) DO UPDATE SET
             base_url = EXCLUDED.base_url, model = EXCLUDED.model,
             api_key_ciphertext = EXCLUDED.api_key_ciphertext,
             api_key_nonce = EXCLUDED.api_key_nonce, api_key_hint = EXCLUDED.api_key_hint,
             enabled = EXCLUDED.enabled, provider_preset = EXCLUDED.provider_preset,
             transport_mode = EXCLUDED.transport_mode,
             thinking_mode = EXCLUDED.thinking_mode,
             reasoning_effort = EXCLUDED.reasoning_effort,
             thinking_budget = EXCLUDED.thinking_budget,
             request_timeout_seconds = EXCLUDED.request_timeout_seconds,
             custom_request_options = EXCLUDED.custom_request_options,
             web_search_mode = EXCLUDED.web_search_mode,
             web_search_provider = EXCLUDED.web_search_provider,
             web_search_endpoint = EXCLUDED.web_search_endpoint,
             web_search_api_key_ciphertext = EXCLUDED.web_search_api_key_ciphertext,
             web_search_api_key_nonce = EXCLUDED.web_search_api_key_nonce,
             web_search_api_key_hint = EXCLUDED.web_search_api_key_hint,
             web_search_timeout_seconds = EXCLUDED.web_search_timeout_seconds,
             web_search_max_results = EXCLUDED.web_search_max_results,
             web_search_citations_enabled = EXCLUDED.web_search_citations_enabled,
             updated_at = now()
         RETURNING *",
    )
    .bind(user_id)
    .bind(base_url)
    .bind(model)
    .bind(ciphertext)
    .bind(nonce)
    .bind(hint)
    .bind(enabled)
    .bind(provider_preset)
    .bind(transport_mode)
    .bind(thinking_mode)
    .bind(reasoning_effort)
    .bind(thinking_budget)
    .bind(request_timeout_seconds)
    .bind(custom_request_options)
    .bind(web_search_mode)
    .bind(web_search_provider)
    .bind(web_search_endpoint)
    .bind(web_search_api_key_ciphertext)
    .bind(web_search_api_key_nonce)
    .bind(web_search_api_key_hint)
    .bind(web_search_timeout_seconds)
    .bind(web_search_max_results)
    .bind(web_search_citations_enabled)
    .fetch_one(conn)
    .await
}

pub async fn delete_user_tx(conn: &mut PgConnection, user_id: i64) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query("DELETE FROM user_ai_settings WHERE user_id = $1")
            .bind(user_id)
            .execute(conn)
            .await?
            .rows_affected()
            == 1,
    )
}

pub async fn find_project(
    pool: &PgPool,
    project_id: i64,
) -> Result<Option<ProjectAiSetting>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM project_ai_settings WHERE project_id = $1")
        .bind(project_id)
        .fetch_optional(pool)
        .await
}

pub async fn find_project_for_update_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<Option<ProjectAiSetting>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM project_ai_settings WHERE project_id = $1 FOR UPDATE")
        .bind(project_id)
        .fetch_optional(conn)
        .await
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_project_tx(
    conn: &mut PgConnection,
    project_id: i64,
    base_url: &str,
    model: &str,
    ciphertext: &[u8],
    nonce: &[u8],
    hint: &str,
    enabled: bool,
    provider_preset: &str,
    transport_mode: &str,
    thinking_mode: &str,
    reasoning_effort: &str,
    thinking_budget: Option<i64>,
    request_timeout_seconds: i32,
    custom_request_options: &serde_json::Value,
    web_search_mode: &str,
    web_search_provider: &str,
    web_search_endpoint: Option<&str>,
    web_search_api_key_ciphertext: Option<&[u8]>,
    web_search_api_key_nonce: Option<&[u8]>,
    web_search_api_key_hint: Option<&str>,
    web_search_timeout_seconds: i32,
    web_search_max_results: i32,
    web_search_citations_enabled: bool,
    actor_id: i64,
) -> Result<ProjectAiSetting, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO project_ai_settings (
             project_id, base_url, model, api_key_ciphertext, api_key_nonce, api_key_hint,
             enabled, provider_preset, transport_mode, thinking_mode, reasoning_effort, thinking_budget,
             request_timeout_seconds, custom_request_options, web_search_mode, web_search_provider,
             web_search_endpoint, web_search_api_key_ciphertext, web_search_api_key_nonce,
             web_search_api_key_hint, web_search_timeout_seconds, web_search_max_results,
             web_search_citations_enabled, updated_by
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24)
         ON CONFLICT (project_id) DO UPDATE SET
             base_url = EXCLUDED.base_url, model = EXCLUDED.model,
             api_key_ciphertext = EXCLUDED.api_key_ciphertext,
             api_key_nonce = EXCLUDED.api_key_nonce, api_key_hint = EXCLUDED.api_key_hint,
             enabled = EXCLUDED.enabled, provider_preset = EXCLUDED.provider_preset,
             transport_mode = EXCLUDED.transport_mode,
             thinking_mode = EXCLUDED.thinking_mode,
             reasoning_effort = EXCLUDED.reasoning_effort,
             thinking_budget = EXCLUDED.thinking_budget,
             request_timeout_seconds = EXCLUDED.request_timeout_seconds,
             custom_request_options = EXCLUDED.custom_request_options,
             web_search_mode = EXCLUDED.web_search_mode,
             web_search_provider = EXCLUDED.web_search_provider,
             web_search_endpoint = EXCLUDED.web_search_endpoint,
             web_search_api_key_ciphertext = EXCLUDED.web_search_api_key_ciphertext,
             web_search_api_key_nonce = EXCLUDED.web_search_api_key_nonce,
             web_search_api_key_hint = EXCLUDED.web_search_api_key_hint,
             web_search_timeout_seconds = EXCLUDED.web_search_timeout_seconds,
             web_search_max_results = EXCLUDED.web_search_max_results,
             web_search_citations_enabled = EXCLUDED.web_search_citations_enabled,
             updated_by = EXCLUDED.updated_by, updated_at = now()
         RETURNING *",
    )
    .bind(project_id)
    .bind(base_url)
    .bind(model)
    .bind(ciphertext)
    .bind(nonce)
    .bind(hint)
    .bind(enabled)
    .bind(provider_preset)
    .bind(transport_mode)
    .bind(thinking_mode)
    .bind(reasoning_effort)
    .bind(thinking_budget)
    .bind(request_timeout_seconds)
    .bind(custom_request_options)
    .bind(web_search_mode)
    .bind(web_search_provider)
    .bind(web_search_endpoint)
    .bind(web_search_api_key_ciphertext)
    .bind(web_search_api_key_nonce)
    .bind(web_search_api_key_hint)
    .bind(web_search_timeout_seconds)
    .bind(web_search_max_results)
    .bind(web_search_citations_enabled)
    .bind(actor_id)
    .fetch_one(conn)
    .await
}

pub async fn delete_project_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query("DELETE FROM project_ai_settings WHERE project_id = $1")
            .bind(project_id)
            .execute(conn)
            .await?
            .rows_affected()
            == 1,
    )
}
