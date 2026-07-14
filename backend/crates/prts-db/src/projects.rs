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
        "SELECT * FROM projects WHERE visibility = 'public'
           AND deletion_scheduled_at IS NULL ORDER BY id DESC LIMIT $1 OFFSET $2",
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
         WHERE m.user_id = $1 AND p.deletion_scheduled_at IS NULL ORDER BY p.id DESC",
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

/// 关联 purge job 并进入待删除状态。
pub async fn schedule_deletion_tx(
    conn: &mut PgConnection,
    id: i64,
    requested_by: i64,
    job_id: i64,
    scheduled_at: chrono::DateTime<chrono::Utc>,
) -> Result<Project, sqlx::Error> {
    sqlx::query_as(
        "UPDATE projects SET deletion_scheduled_at = $2, deletion_requested_by = $3,
             deletion_job_id = $4 WHERE id = $1 AND deletion_scheduled_at IS NULL RETURNING *",
    )
    .bind(id)
    .bind(scheduled_at)
    .bind(requested_by)
    .bind(job_id)
    .fetch_one(conn)
    .await
}

/// 取消未到期的待删除状态；调用方已锁定关联 job。
pub async fn cancel_deletion_tx(conn: &mut PgConnection, id: i64) -> Result<Project, sqlx::Error> {
    sqlx::query_as(
        "UPDATE projects SET deletion_scheduled_at = NULL, deletion_requested_by = NULL,
             deletion_job_id = NULL WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .fetch_one(conn)
    .await
}

/// worker 查询全部 pending project id，用于暂停普通任务。
pub async fn pending_deletion_ids(pool: &PgPool) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM projects WHERE deletion_scheduled_at IS NOT NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await
}

/// 项目 purge 前置空跨域 live refs，并取消上传活动。
pub async fn detach_live_refs_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE task_baseline_entries SET live_entry_id = NULL WHERE live_entry_id IN (SELECT id FROM entries WHERE project_id = $1)").bind(project_id).execute(&mut *conn).await?;
    sqlx::query("UPDATE task_files SET live_file_id = NULL WHERE live_file_id IN (SELECT id FROM files WHERE project_id = $1)").bind(project_id).execute(&mut *conn).await?;
    sqlx::query("UPDATE language_resolution_issues SET entry_id = NULL WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE upload_file_attempts SET state = CASE WHEN state IN ('uploading','receiving','queued','processing') THEN 'cancelled' ELSE state END, target_file_id = NULL, finished_at = COALESCE(finished_at, now()) WHERE batch_file_id IN (SELECT file.id FROM upload_batch_files file JOIN upload_batches batch ON batch.id = file.batch_id WHERE batch.project_id_snapshot = $1)").bind(project_id).execute(&mut *conn).await?;
    sqlx::query("UPDATE upload_batch_files SET target_file_id = NULL WHERE batch_id IN (SELECT id FROM upload_batches WHERE project_id_snapshot = $1)").bind(project_id).execute(&mut *conn).await?;
    Ok(())
}

/// 搜索/vector 与 entries 同行，先显式清 versions、统计与 search payload。
pub async fn delete_entry_versions_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM entry_versions WHERE entry_id IN (SELECT id FROM entries WHERE project_id = $1)").bind(project_id).execute(&mut *conn).await?;
    sqlx::query("DELETE FROM file_stats WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM project_stats WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE entries SET embedding = NULL, source_tsv = NULL, translation_tsv = NULL WHERE project_id = $1").bind(project_id).execute(&mut *conn).await?;
    Ok(())
}

/// 叶到根删除 entry/file/folder；history target 暂时锚定到不可提交的 folder。
pub async fn delete_entries_files_folders_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<Option<i64>, sqlx::Error> {
    let history_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM file_change_sets WHERE project_id = $1)")
            .bind(project_id)
            .fetch_one(&mut *conn)
            .await?;
    let anchor = if history_exists {
        let name = format!(
            "__prts_project_purge_anchor_{}",
            uuid::Uuid::new_v4().simple()
        );
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO folders (project_id, name, path) VALUES ($1,$2,$2) RETURNING id",
        )
        .bind(project_id)
        .bind(name)
        .fetch_one(&mut *conn)
        .await?;
        sqlx::query(
            "UPDATE file_change_sets SET file_id = NULL, folder_id = $2 WHERE project_id = $1",
        )
        .bind(project_id)
        .bind(id)
        .execute(&mut *conn)
        .await?;
        Some(id)
    } else {
        None
    };
    sqlx::query("DELETE FROM entries WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM files WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    let folder_ids: Vec<i64> = sqlx::query_scalar(
        "WITH RECURSIVE tree AS (
             SELECT id, 0 AS depth FROM folders WHERE project_id=$1 AND parent_id IS NULL
             UNION ALL
             SELECT child.id, tree.depth + 1 FROM folders child JOIN tree ON child.parent_id=tree.id
             WHERE child.project_id=$1
         ) SELECT id FROM tree WHERE id IS DISTINCT FROM $2 ORDER BY depth DESC, id",
    )
    .bind(project_id)
    .bind(anchor)
    .fetch_all(&mut *conn)
    .await?;
    for folder_id in folder_ids {
        sqlx::query("DELETE FROM folders WHERE id=$1")
            .bind(folder_id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(anchor)
}

/// schedule 时复制所有尚未清理的 upload temp key，purge 后无需数据库行即可重试。
pub async fn purge_temp_keys_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT attempt.temp_key FROM upload_file_attempts attempt
         JOIN upload_batch_files file ON file.id=attempt.batch_file_id
         JOIN upload_batches batch ON batch.id=file.batch_id
         WHERE batch.project_id_snapshot=$1 AND attempt.cleaned_at IS NULL
         ORDER BY attempt.id",
    )
    .bind(project_id)
    .fetch_all(conn)
    .await
}

pub async fn delete_file_history_tx(
    conn: &mut PgConnection,
    project_id: i64,
    anchor: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM file_change_items WHERE change_set_id IN (SELECT id FROM file_change_sets WHERE project_id = $1)").bind(project_id).execute(&mut *conn).await?;
    sqlx::query("DELETE FROM file_change_sets WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    if let Some(anchor) = anchor {
        sqlx::query("DELETE FROM folders WHERE id = $1")
            .bind(anchor)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

pub async fn delete_tasks_tx(conn: &mut PgConnection, project_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM task_stats WHERE task_id IN (SELECT id FROM tasks WHERE project_id = $1)",
    )
    .bind(project_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM task_baseline_entries WHERE task_file_id IN (SELECT tf.id FROM task_files tf JOIN tasks t ON t.id=tf.task_id WHERE t.project_id=$1)").bind(project_id).execute(&mut *conn).await?;
    sqlx::query(
        "DELETE FROM task_files WHERE task_id IN (SELECT id FROM tasks WHERE project_id=$1)",
    )
    .bind(project_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM tasks WHERE project_id=$1")
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn delete_terms_tx(conn: &mut PgConnection, project_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM terms WHERE project_id=$1")
        .bind(project_id)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn delete_project_metadata_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM language_resolution_issues WHERE project_id=$1")
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM upload_batches WHERE project_id_snapshot=$1")
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("SET CONSTRAINTS memberships_owner_guard_trg DEFERRED")
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM memberships WHERE project_id=$1")
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn delete_project_row_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query("DELETE FROM projects WHERE id=$1")
        .bind(project_id)
        .execute(conn)
        .await
        .map(|result| result.rows_affected() == 1)
}

/// 测试夹具清理项目；生产业务必须使用 24 小时 project_purge workflow。
#[doc(hidden)]
pub async fn delete_test_fixture(pool: &PgPool, id: i64) -> Result<bool, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    delete_test_fixture_tx(&mut connection, id).await
}

/// 测试夹具事务内清理；不得从生产 route 调用。
///
/// `0010` 的 file history 对 project 使用 RESTRICT，不能再依赖项目级模糊 cascade。
/// 冻结 schema 的 target CHECK 与 SET NULL 冲突，故事务内先用不可见 anchor 承接 target，
/// 再删业务树、history payload、anchor 与项目；anchor 状态绝不提交。
#[doc(hidden)]
pub async fn delete_test_fixture_tx(conn: &mut PgConnection, id: i64) -> Result<bool, sqlx::Error> {
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
