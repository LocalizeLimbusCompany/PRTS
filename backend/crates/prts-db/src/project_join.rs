//! Project self-service join applications and safe member candidate lookup.

use sqlx::{PgConnection, PgPool};

use crate::models::{ProjectJoinApplication, ProjectJoinApplicationInfo, UserCandidate};

/// Create one pending application; the partial unique index closes concurrent duplicates.
pub async fn create_application_tx(
    conn: &mut PgConnection,
    project_id: i64,
    user_id: i64,
    message: &str,
) -> Result<Option<ProjectJoinApplication>, sqlx::Error> {
    sqlx::query_as::<_, ProjectJoinApplication>(
        "INSERT INTO project_join_applications (project_id, user_id, message)
         VALUES ($1, $2, $3)
         ON CONFLICT (project_id, user_id) WHERE status = 'pending' DO NOTHING
         RETURNING *",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(message)
    .fetch_optional(conn)
    .await
}

/// Read the caller's current pending application.
pub async fn pending_for_user(
    pool: &PgPool,
    project_id: i64,
    user_id: i64,
) -> Result<Option<ProjectJoinApplication>, sqlx::Error> {
    sqlx::query_as::<_, ProjectJoinApplication>(
        "SELECT * FROM project_join_applications
         WHERE project_id = $1 AND user_id = $2 AND status = 'pending'",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

/// List pending applications by descending id without a count query.
pub async fn list_pending(
    pool: &PgPool,
    project_id: i64,
    after: Option<i64>,
    limit: i64,
) -> Result<Vec<ProjectJoinApplicationInfo>, sqlx::Error> {
    sqlx::query_as::<_, ProjectJoinApplicationInfo>(
        "SELECT application.id, application.project_id, application.user_id,
                users.username, users.avatar_url, application.status,
                application.message, application.created_at
         FROM project_join_applications AS application
         JOIN users ON users.id = application.user_id
         WHERE application.project_id = $1 AND application.status = 'pending'
           AND ($2::BIGINT IS NULL OR application.id < $2)
         ORDER BY application.id DESC LIMIT $3",
    )
    .bind(project_id)
    .bind(after)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Lock one pending application after the project row has serialized membership changes.
pub async fn find_pending_for_update_tx(
    conn: &mut PgConnection,
    project_id: i64,
    application_id: i64,
) -> Result<Option<ProjectJoinApplication>, sqlx::Error> {
    sqlx::query_as::<_, ProjectJoinApplication>(
        "SELECT * FROM project_join_applications
         WHERE id = $1 AND project_id = $2 AND status = 'pending' FOR UPDATE",
    )
    .bind(application_id)
    .bind(project_id)
    .fetch_optional(conn)
    .await
}

/// Withdraw the caller's pending application.
pub async fn withdraw_tx(
    conn: &mut PgConnection,
    project_id: i64,
    user_id: i64,
) -> Result<Option<ProjectJoinApplication>, sqlx::Error> {
    sqlx::query_as::<_, ProjectJoinApplication>(
        "UPDATE project_join_applications SET status = 'withdrawn'
         WHERE project_id = $1 AND user_id = $2 AND status = 'pending' RETURNING *",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(conn)
    .await
}

/// Finish an application after the optional membership write has succeeded.
pub async fn decide_tx(
    conn: &mut PgConnection,
    application_id: i64,
    actor_id: i64,
    approved: bool,
    role: Option<&str>,
) -> Result<ProjectJoinApplication, sqlx::Error> {
    sqlx::query_as::<_, ProjectJoinApplication>(
        "UPDATE project_join_applications
         SET status = CASE WHEN $3 THEN 'approved' ELSE 'rejected' END,
             decided_by = $2, decided_role = $4, decided_at = now()
         WHERE id = $1 RETURNING *",
    )
    .bind(application_id)
    .bind(actor_id)
    .bind(approved)
    .bind(role)
    .fetch_one(conn)
    .await
}

/// Manager notification recipients for a new application.
pub async fn manager_ids_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT user_id FROM memberships
         WHERE project_id = $1 AND role IN ('owner', 'manager') ORDER BY user_id",
    )
    .bind(project_id)
    .fetch_all(conn)
    .await
}

/// UID exact lookup or debounced username-prefix lookup, excluding existing members.
pub async fn member_candidates(
    pool: &PgPool,
    project_id: i64,
    query: &str,
) -> Result<Vec<UserCandidate>, sqlx::Error> {
    if let Ok(user_id) = query.parse::<i64>() {
        return sqlx::query_as::<_, UserCandidate>(
            "SELECT users.id AS user_id, users.username, users.avatar_url
             FROM users
             WHERE users.id = $1 AND users.status = 'active'
               AND NOT EXISTS (
                   SELECT 1 FROM memberships
                   WHERE memberships.project_id = $2 AND memberships.user_id = users.id
               ) LIMIT 1",
        )
        .bind(user_id)
        .bind(project_id)
        .fetch_all(pool)
        .await;
    }
    sqlx::query_as::<_, UserCandidate>(
        "SELECT users.id AS user_id, users.username, users.avatar_url
         FROM users
         WHERE users.status = 'active'
           AND users.username ILIKE prts_escape_like_pattern($2) || '%' ESCAPE '\\'
           AND NOT EXISTS (
               SELECT 1 FROM memberships
               WHERE memberships.project_id = $1 AND memberships.user_id = users.id
           )
         ORDER BY similarity(users.username, $2) DESC, users.id ASC LIMIT 20",
    )
    .bind(project_id)
    .bind(query)
    .fetch_all(pool)
    .await
}
