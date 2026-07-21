//! Project-wide entry-version timeline with visibility policy and stable keyset pagination.

use axum::extract::{Path, Query, State};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use prts_common::Error;
use prts_core::permission::nodes;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::auth::{project as paccess, MaybeUser};
use crate::db_err;
use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

#[derive(Debug, Deserialize, IntoParams)]
pub struct ProjectHistoryQuery {
    pub after: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EntryFieldChangeDto {
    pub field: String,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectHistoryItemDto {
    pub id: i64,
    pub entry_id: i64,
    pub file_id: i64,
    pub file_path: String,
    pub entry_key: String,
    pub kind: String,
    pub editor_id: Option<i64>,
    pub editor_name: Option<String>,
    pub editor_avatar_url: Option<String>,
    pub created_at: String,
    pub original: Option<serde_json::Value>,
    pub translation: Option<String>,
    pub state: Option<String>,
    pub questioned: Option<bool>,
    pub locked: bool,
    pub hidden: bool,
    pub changes: Vec<EntryFieldChangeDto>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectHistoryPageDto {
    pub items: Vec<ProjectHistoryItemDto>,
    pub next_after: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct HistoryCursor {
    version: u8,
    project_id: i64,
    created_at: chrono::DateTime<chrono::Utc>,
    id: i64,
}

#[utoipa::path(get, path = "/projects/{id}/history", tag = "project-history",
    params(ProjectHistoryQuery),
    description = "List concrete entry changes newest-first using a `(created_at,id)` keyset. Visibility follows the project's viewers/members/managers policy and each item contains file/entry deep-link identifiers.",
    responses((status = 200, body = ProjectHistoryPageDto), (status = 400, body = ErrorResponse),
        (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse)))]
pub async fn project_history(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
    Query(query): Query<ProjectHistoryQuery>,
) -> Result<Json<ProjectHistoryPageDto>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;
    match access.project.history_visibility.as_str() {
        "viewers" => {}
        "members" if !access.is_project_member() => return Err(Error::Forbidden.into()),
        "managers" if !access.has_node(nodes::PROJECT_MANAGE) => {
            return Err(Error::Forbidden.into())
        }
        "members" | "managers" => {}
        _ => return Err(Error::internal("invalid project history visibility").into()),
    }
    let after = query
        .after
        .as_deref()
        .map(|cursor| decode_cursor(cursor, id))
        .transpose()?;
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let rows = prts_db::entries::list_project_versions(
        &state.db,
        id,
        after.map(|cursor| (cursor.created_at, cursor.id)),
        limit,
    )
    .await
    .map_err(db_err)?;
    let next_after = if rows.len() as i64 == limit {
        rows.last().map(|row| {
            encode_cursor(&HistoryCursor {
                version: 1,
                project_id: id,
                created_at: row.created_at,
                id: row.id,
            })
        })
    } else {
        None
    };
    Ok(Json(ProjectHistoryPageDto {
        items: rows.into_iter().map(history_item).collect(),
        next_after,
    }))
}

fn history_item(row: prts_db::models::ProjectEntryVersion) -> ProjectHistoryItemDto {
    let mut changes = Vec::with_capacity(6);
    push_change(
        &mut changes,
        "original",
        row.previous_original
            .clone()
            .map_or(serde_json::Value::Null, |value| value),
        row.original
            .clone()
            .map_or(serde_json::Value::Null, |value| value),
    );
    push_change(
        &mut changes,
        "translation",
        serde_json::json!(row.previous_translation),
        serde_json::json!(row.translation),
    );
    push_change(
        &mut changes,
        "state",
        serde_json::json!(row.previous_state),
        serde_json::json!(row.state),
    );
    push_change(
        &mut changes,
        "questioned",
        serde_json::json!(row.previous_questioned),
        serde_json::json!(row.questioned),
    );
    push_change(
        &mut changes,
        "locked",
        serde_json::json!(row.previous_locked),
        serde_json::json!(row.locked),
    );
    push_change(
        &mut changes,
        "hidden",
        serde_json::json!(row.previous_hidden),
        serde_json::json!(row.hidden),
    );
    ProjectHistoryItemDto {
        id: row.id,
        entry_id: row.entry_id,
        file_id: row.file_id,
        file_path: row.file_path,
        entry_key: row.entry_key,
        kind: row.kind,
        editor_id: row.editor_id,
        editor_name: row.editor_name,
        editor_avatar_url: row.editor_avatar_url,
        created_at: row.created_at.to_rfc3339(),
        original: row.original,
        translation: row.translation,
        state: row.state,
        questioned: row.questioned,
        locked: row.locked,
        hidden: row.hidden,
        changes,
    }
}

fn push_change(
    changes: &mut Vec<EntryFieldChangeDto>,
    field: &str,
    before: serde_json::Value,
    after: serde_json::Value,
) {
    if before != after {
        changes.push(EntryFieldChangeDto {
            field: field.to_string(),
            before,
            after,
        });
    }
}

fn encode_cursor(cursor: &HistoryCursor) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(cursor).expect("history cursor serializes"))
}

fn decode_cursor(value: &str, project_id: i64) -> Result<HistoryCursor, ApiError> {
    let cursor = URL_SAFE_NO_PAD
        .decode(value)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<HistoryCursor>(&bytes).ok())
        .filter(|cursor| cursor.version == 1 && cursor.project_id == project_id && cursor.id > 0)
        .ok_or_else(|| Error::validation("PROJECT_HISTORY_CURSOR_INVALID"))?;
    Ok(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_is_bound_to_project() {
        let value = encode_cursor(&HistoryCursor {
            version: 1,
            project_id: 7,
            created_at: chrono::Utc::now(),
            id: 9,
        });
        assert!(decode_cursor(&value, 7).is_ok());
        assert!(decode_cursor(&value, 8).is_err());
    }
}
