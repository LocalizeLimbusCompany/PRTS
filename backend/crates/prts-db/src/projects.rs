//! 项目数据访问。

use sqlx::{PgConnection, PgPool};

use crate::models::Project;

/// 创建项目。
#[allow(clippy::too_many_arguments)]
pub async fn create(
    pool: &PgPool,
    slug: &str,
    name: &str,
    description: &str,
    visibility: &str,
    source_langs: &[String],
    target_lang: &str,
    owner_id: i64,
) -> Result<Project, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    create_tx(
        &mut connection,
        slug,
        name,
        description,
        visibility,
        source_langs,
        target_lang,
        owner_id,
    )
    .await
}

/// 在调用方事务内创建项目。
#[allow(clippy::too_many_arguments)]
pub async fn create_tx(
    conn: &mut PgConnection,
    slug: &str,
    name: &str,
    description: &str,
    visibility: &str,
    source_langs: &[String],
    target_lang: &str,
    owner_id: i64,
) -> Result<Project, sqlx::Error> {
    sqlx::query_as::<_, Project>(
        "INSERT INTO projects (slug, name, description, visibility, source_langs, target_lang, owner_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING *",
    )
    .bind(slug)
    .bind(name)
    .bind(description)
    .bind(visibility)
    .bind(source_langs)
    .bind(target_lang)
    .bind(owner_id)
    .fetch_one(conn)
    .await
}

/// 在调用方事务内以显式 canonical 主源创建项目。
#[allow(clippy::too_many_arguments)]
pub async fn create_with_primary_tx(
    conn: &mut PgConnection,
    slug: &str,
    name: &str,
    description: &str,
    visibility: &str,
    source_langs: &[String],
    primary_source_lang: &str,
    target_lang: &str,
    owner_id: i64,
) -> Result<Project, sqlx::Error> {
    sqlx::query_as::<_, Project>(
        "INSERT INTO projects (
             slug, name, description, visibility, source_langs,
             primary_source_lang, target_lang, owner_id
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING *",
    )
    .bind(slug)
    .bind(name)
    .bind(description)
    .bind(visibility)
    .bind(source_langs)
    .bind(primary_source_lang)
    .bind(target_lang)
    .bind(owner_id)
    .fetch_one(conn)
    .await
}

/// 按 id 查找。
pub async fn find_by_id(pool: &PgPool, id: i64) -> Result<Option<Project>, sqlx::Error> {
    sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// 在调用方事务内锁定项目并返回一致快照。
pub async fn find_by_id_for_update_tx(
    conn: &mut PgConnection,
    id: i64,
) -> Result<Option<Project>, sqlx::Error> {
    sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// 按 slug 查找。
pub async fn find_by_slug(pool: &PgPool, slug: &str) -> Result<Option<Project>, sqlx::Error> {
    sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE slug = $1")
        .bind(slug)
        .fetch_optional(pool)
        .await
}

/// slug 是否已存在。
pub async fn slug_exists(pool: &PgPool, slug: &str) -> Result<bool, sqlx::Error> {
    let (exists,): (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM projects WHERE slug = $1)")
            .bind(slug)
            .fetch_one(pool)
            .await?;
    Ok(exists)
}

/// 列出公开项目（分页）。
pub async fn list_public(
    pool: &PgPool,
    limit: i64,
    offset: i64,
) -> Result<Vec<Project>, sqlx::Error> {
    sqlx::query_as::<_, Project>(
        "SELECT * FROM projects WHERE visibility = 'public' ORDER BY id DESC LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

/// 列出某用户参与的项目（含私有）。
pub async fn list_for_user(pool: &PgPool, user_id: i64) -> Result<Vec<Project>, sqlx::Error> {
    sqlx::query_as::<_, Project>(
        "SELECT p.* FROM projects p
         JOIN memberships m ON m.project_id = p.id
         WHERE m.user_id = $1 ORDER BY p.id DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// 更新项目元信息。
#[allow(clippy::too_many_arguments)]
pub async fn update(
    pool: &PgPool,
    id: i64,
    name: &str,
    description: &str,
    visibility: &str,
    source_langs: &[String],
    target_lang: &str,
) -> Result<Project, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    update_tx(
        &mut connection,
        id,
        name,
        description,
        visibility,
        source_langs,
        target_lang,
    )
    .await
}

/// 在调用方事务内更新项目元信息。
#[allow(clippy::too_many_arguments)]
pub async fn update_tx(
    conn: &mut PgConnection,
    id: i64,
    name: &str,
    description: &str,
    visibility: &str,
    source_langs: &[String],
    target_lang: &str,
) -> Result<Project, sqlx::Error> {
    sqlx::query_as::<_, Project>(
        "UPDATE projects SET name = $2, description = $3, visibility = $4,
             source_langs = $5, target_lang = $6
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(visibility)
    .bind(source_langs)
    .bind(target_lang)
    .fetch_one(conn)
    .await
}

/// 在调用方事务内更新项目头像元数据。
pub async fn set_avatar_tx(
    conn: &mut PgConnection,
    id: i64,
    key: &str,
    content_type: &str,
) -> Result<Project, sqlx::Error> {
    sqlx::query_as::<_, Project>(
        "UPDATE projects
         SET avatar_key = $2, avatar_content_type = $3, avatar_updated_at = now()
         WHERE id = $1
         RETURNING *",
    )
    .bind(id)
    .bind(key)
    .bind(content_type)
    .fetch_one(conn)
    .await
}

/// 在调用方事务内清除项目头像元数据。
pub async fn clear_avatar_tx(conn: &mut PgConnection, id: i64) -> Result<Project, sqlx::Error> {
    sqlx::query_as::<_, Project>(
        "UPDATE projects
         SET avatar_key = NULL, avatar_content_type = NULL, avatar_updated_at = NULL
         WHERE id = $1
         RETURNING *",
    )
    .bind(id)
    .fetch_one(conn)
    .await
}

/// 原子切换主源并关联本次词法重建任务。
pub async fn change_primary_source_tx(
    conn: &mut PgConnection,
    id: i64,
    source_langs: &[String],
    primary_source_lang: &str,
    lexical_job_id: i64,
) -> Result<Project, sqlx::Error> {
    sqlx::query_as::<_, Project>(
        "UPDATE projects
         SET source_langs = $2, primary_source_lang = $3,
             primary_source_changed_at = now(),
             lexical_state = 'rebuilding', lexical_job_id = $4,
             embedding_state = 'pending', embedding_job_id = NULL
         WHERE id = $1
         RETURNING *",
    )
    .bind(id)
    .bind(source_langs)
    .bind(primary_source_lang)
    .bind(lexical_job_id)
    .fetch_one(conn)
    .await
}

/// 删除项目（级联文件夹/文件/词条/成员）。
pub async fn delete(pool: &PgPool, id: i64) -> Result<bool, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    delete_tx(&mut connection, id).await
}

/// 在调用方事务内执行 Task 7 延迟清除上线前的兼容立即删除。
///
/// `0010` 的 file history 对 project 使用 RESTRICT，不能再依赖项目级模糊 cascade。
/// 冻结 schema 的 target CHECK 与 SET NULL 冲突，故事务内先用不可见 anchor 承接 target，
/// 再删业务树、history payload、anchor 与项目；anchor 状态绝不提交。
pub async fn delete_tx(conn: &mut PgConnection, id: i64) -> Result<bool, sqlx::Error> {
    let history_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM file_change_sets WHERE project_id = $1)")
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
    let purge_anchor_id = if history_exists {
        let anchor_name = format!(
            "__prts_internal_project_purge_anchor_{}",
            uuid::Uuid::new_v4().simple()
        );
        let anchor_id: i64 = sqlx::query_scalar(
            "INSERT INTO folders (project_id, parent_id, name, path)
             VALUES ($1, NULL, $2, $2) RETURNING id",
        )
        .bind(id)
        .bind(&anchor_name)
        .fetch_one(&mut *conn)
        .await?;
        sqlx::query(
            "UPDATE file_change_sets SET file_id = NULL, folder_id = $2 WHERE project_id = $1",
        )
        .bind(id)
        .bind(anchor_id)
        .execute(&mut *conn)
        .await?;
        Some(anchor_id)
    } else {
        None
    };
    sqlx::query("DELETE FROM files WHERE project_id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM folders WHERE project_id = $1 AND id IS DISTINCT FROM $2")
        .bind(id)
        .bind(purge_anchor_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "DELETE FROM file_change_items
         WHERE change_set_id IN (SELECT id FROM file_change_sets WHERE project_id = $1)",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM file_change_sets WHERE project_id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    if let Some(anchor_id) = purge_anchor_id {
        sqlx::query("DELETE FROM folders WHERE id = $1")
            .bind(anchor_id)
            .execute(&mut *conn)
            .await?;
    }
    let res = sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(res.rows_affected() > 0)
}
