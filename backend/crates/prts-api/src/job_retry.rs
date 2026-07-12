//! 手动重试任务的事务编排。
//!
//! 该模块固定 project→job 锁序，并在锁内重新加载权限；HTTP route 只负责协议映射。

use prts_common::Error;

use crate::auth::{project as paccess, CurrentUser};
use crate::db_err;
use crate::error::ApiError;
use crate::state::AppState;

/// 手动重试失败任务，并将业务状态与 allowlisted 审计原子提交。
pub(crate) async fn retry_job(
    state: &AppState,
    user: &CurrentUser,
    id: i64,
) -> Result<prts_db::models::Job, ApiError> {
    let snapshot = prts_db::jobs::find_by_id(&state.db, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    prts_core::jobs::job_view_policy(&snapshot.kind).ok_or(Error::NotFound)?;
    let project_id = snapshot.project_id.ok_or(Error::NotFound)?;

    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, project_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let access = paccess::load_locked_tx(&mut tx, user, project).await?;
    let current = prts_db::jobs::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    if current.project_id != Some(project_id)
        || prts_core::jobs::job_view_policy(&current.kind).is_none()
    {
        return Err(Error::NotFound.into());
    }
    access.require_view()?;
    if !can_view_job(&access, user, &current.kind) {
        return Err(Error::Forbidden.into());
    }
    let retry_policy = prts_core::jobs::manual_retry_policy(&current.kind)
        .ok_or_else(|| Error::bad_request("manual retry is not supported for this job kind"))?;
    if retry_policy.owner_only && access.project.owner_id != user.id {
        return Err(Error::Forbidden.into());
    }
    access.require_node(retry_policy.permission_node)?;
    let state_value = prts_core::JobState::parse(&current.state)
        .ok_or_else(|| Error::internal("unknown job state persisted"))?;
    if !state_value.manual_retry_allowed() {
        return Err(Error::bad_request("job is not manually retryable").into());
    }
    let updated = prts_db::jobs::manual_retry_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| Error::bad_request("job state changed; retry rejected"))?;
    match updated.kind.as_str() {
        "primary_source_lexical_reindex" => {
            sqlx::query(
                "UPDATE projects SET lexical_state = 'rebuilding'
                 WHERE id = $1 AND lexical_job_id = $2",
            )
            .bind(project_id)
            .bind(updated.id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        "primary_source_embedding_backfill" => {
            sqlx::query(
                "UPDATE projects SET embedding_state = 'running'
                 WHERE id = $1 AND embedding_job_id = $2",
            )
            .bind(project_id)
            .bind(updated.id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        _ => {}
    }
    prts_db::audit::append_job_retried_tx(
        &mut tx,
        prts_db::audit::AuditActor {
            id: Some(user.id),
            kind: prts_db::audit::AuditActorKind::User,
            ip: None,
        },
        updated.id,
        updated.project_id,
        &updated.kind,
        current.attempts,
        updated.attempts,
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    state.job_worker.wake();
    Ok(updated)
}

fn can_view_job(access: &paccess::ProjectAccess, user: &CurrentUser, kind: &str) -> bool {
    match prts_core::jobs::job_view_policy(kind) {
        Some(policy) if policy.owner_only => access.project.owner_id == user.id,
        Some(policy) => access.has_node(policy.permission_node),
        None => false,
    }
}
