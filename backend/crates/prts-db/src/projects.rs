//! 项目数据访问。

use sqlx::PgPool;

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
    .fetch_one(pool)
    .await
}

/// 按 id 查找。
pub async fn find_by_id(pool: &PgPool, id: i64) -> Result<Option<Project>, sqlx::Error> {
    sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
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
    .fetch_one(pool)
    .await
}

/// 删除项目（级联文件夹/文件/词条/成员）。
pub async fn delete(pool: &PgPool, id: i64) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}
