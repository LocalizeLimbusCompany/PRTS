//! 项目任务、immutable baseline 与物化进度仓储。
//!
//! 本模块只执行 [`prts_core::tasks`] 生成的 typed file-set plan。任务列表使用 id 键集，
//! baseline/progress 查询复用数据库中的规范 effective-visible 谓词。

use chrono::{DateTime, Utc};
use prts_core::tasks::{TaskEntrySnapshot, TaskFileSetPlan, TaskProgressDelta};
use prts_core::EntryState;
use sqlx::{FromRow, PgConnection, PgPool};

use crate::models::{Task, TaskStats};

/// 任务列表一行；不携带 Markdown 正文。
#[derive(Debug, Clone, FromRow)]
pub struct TaskListItem {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub created_by: Option<i64>,
    pub denominator: i64,
    pub completed: i64,
    pub file_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 任务文件及其当前可解释 live metadata。
#[derive(Debug, Clone, FromRow)]
pub struct TaskFileDetail {
    pub id: i64,
    pub file_id_snapshot: i64,
    pub live_file_id: Option<i64>,
    pub name: Option<String>,
    pub path: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// 文件集合 plan 的持久化结果，只含审计可安全记录的计数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskFileSetApplyResult {
    pub retained_files: usize,
    pub added_files: usize,
    pub removed_files: usize,
    pub baseline_entries_added: i64,
}

/// 新建任务元数据；task_stats 由迁移 trigger 同事务初始化为零。
pub async fn create_tx(
    conn: &mut PgConnection,
    project_id: i64,
    actor_id: i64,
    title: &str,
    description: &str,
) -> Result<Task, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO tasks (project_id, title, description, created_by)
         VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(project_id)
    .bind(title)
    .bind(description)
    .bind(actor_id)
    .fetch_one(conn)
    .await
}

/// 更新任务元数据；URL project 与 task 归属在同一条件中 fail closed。
pub async fn update_metadata_tx(
    conn: &mut PgConnection,
    project_id: i64,
    task_id: i64,
    title: &str,
    description: &str,
) -> Result<Option<Task>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE tasks SET title = $3, description = $4
         WHERE id = $1 AND project_id = $2 RETURNING *",
    )
    .bind(task_id)
    .bind(project_id)
    .bind(title)
    .bind(description)
    .fetch_optional(conn)
    .await
}

/// 删除任务；task_file/baseline/stats 只在显式删除任务时级联。
pub async fn delete_tx(
    conn: &mut PgConnection,
    project_id: i64,
    task_id: i64,
) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query("DELETE FROM tasks WHERE id = $1 AND project_id = $2")
            .bind(task_id)
            .bind(project_id)
            .execute(conn)
            .await?
            .rows_affected()
            == 1,
    )
}

/// 删除审计所需的安全计数；不读取标题或 Markdown 正文。
pub async fn snapshot_counts_tx(
    conn: &mut PgConnection,
    project_id: i64,
    task_id: i64,
) -> Result<Option<(i64, i64)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT count(DISTINCT task_file.id) AS file_count,
                count(baseline.id) AS baseline_entry_count
         FROM tasks AS task
         LEFT JOIN task_files AS task_file ON task_file.task_id = task.id
         LEFT JOIN task_baseline_entries AS baseline
           ON baseline.task_file_id = task_file.id
         WHERE task.id = $1 AND task.project_id = $2
         GROUP BY task.id",
    )
    .bind(task_id)
    .bind(project_id)
    .fetch_optional(conn)
    .await
}

/// 按 URL project 查任务；跨项目 task id 与不存在统一返回 None。
pub async fn find(
    pool: &PgPool,
    project_id: i64,
    task_id: i64,
) -> Result<Option<Task>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM tasks WHERE id = $1 AND project_id = $2")
        .bind(task_id)
        .bind(project_id)
        .fetch_optional(pool)
        .await
}

/// 锁定 URL project 下的任务，供 mutation 事务重新校验归属。
pub async fn find_for_update_tx(
    conn: &mut PgConnection,
    project_id: i64,
    task_id: i64,
) -> Result<Option<Task>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM tasks WHERE id = $1 AND project_id = $2 FOR UPDATE")
        .bind(task_id)
        .bind(project_id)
        .fetch_optional(conn)
        .await
}

/// 任务列表键集分页，稳定顺序为 id DESC。
pub async fn list(
    pool: &PgPool,
    project_id: i64,
    after: Option<i64>,
    limit: i64,
) -> Result<Vec<TaskListItem>, sqlx::Error> {
    sqlx::query_as(
        "SELECT task.id, task.project_id, task.title, task.created_by,
                stats.denominator, stats.completed,
                count(task_file.id) AS file_count,
                task.created_at, task.updated_at
         FROM tasks AS task
         JOIN task_stats AS stats ON stats.task_id = task.id
         LEFT JOIN task_files AS task_file ON task_file.task_id = task.id
         WHERE task.project_id = $1 AND ($2::BIGINT IS NULL OR task.id < $2)
         GROUP BY task.id, stats.task_id
         ORDER BY task.id DESC
         LIMIT $3",
    )
    .bind(project_id)
    .bind(after)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 读取任务物化统计并绑定 URL project。
pub async fn stats(
    pool: &PgPool,
    project_id: i64,
    task_id: i64,
) -> Result<Option<TaskStats>, sqlx::Error> {
    sqlx::query_as(
        "SELECT stats.* FROM task_stats AS stats
         JOIN tasks AS task ON task.id = stats.task_id
         WHERE task.id = $1 AND task.project_id = $2",
    )
    .bind(task_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await
}

/// 读取任务文件详情；purge 后 path/name 为空但 immutable snapshot id 保留。
pub async fn file_details(
    pool: &PgPool,
    project_id: i64,
    task_id: i64,
) -> Result<Option<Vec<TaskFileDetail>>, sqlx::Error> {
    if find(pool, project_id, task_id).await?.is_none() {
        return Ok(None);
    }
    sqlx::query_as(
        "SELECT task_file.id, task_file.file_id_snapshot, task_file.live_file_id,
                file.name, file.path, task_file.created_at
         FROM task_files AS task_file
         LEFT JOIN files AS file ON file.id = task_file.live_file_id
         WHERE task_file.task_id = $1
         ORDER BY task_file.id",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map(Some)
}

/// 测试/verify 用：读取 immutable baseline entry IDs，不返回词条正文。
pub async fn baseline_entry_ids(
    pool: &PgPool,
    project_id: i64,
    task_id: i64,
) -> Result<Option<Vec<i64>>, sqlx::Error> {
    if find(pool, project_id, task_id).await?.is_none() {
        return Ok(None);
    }
    sqlx::query_scalar(
        "SELECT baseline.entry_id_snapshot
         FROM task_baseline_entries AS baseline
         JOIN task_files AS task_file ON task_file.id = baseline.task_file_id
         WHERE task_file.task_id = $1
         ORDER BY baseline.entry_id_snapshot",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map(Some)
}

/// 锁定并返回当前具有 live ref 的 task file IDs。
pub async fn active_file_ids_tx(
    conn: &mut PgConnection,
    project_id: i64,
    task_id: i64,
) -> Result<Option<Vec<i64>>, sqlx::Error> {
    if find_for_update_tx(conn, project_id, task_id)
        .await?
        .is_none()
    {
        return Ok(None);
    }
    sqlx::query_scalar(
        "SELECT live_file_id FROM task_files
         WHERE task_id = $1 AND live_file_id IS NOT NULL
         ORDER BY live_file_id FOR UPDATE",
    )
    .bind(task_id)
    .fetch_all(conn)
    .await
    .map(Some)
}

/// 执行 core 的完整期望文件集合 plan。
///
/// 新增文件时使用规范 SQL predicate 建立 baseline；保留项不重建，移除项显式删除
/// task_file 并由声明的 CASCADE 删除其 baseline。
pub async fn apply_file_set_plan_tx(
    conn: &mut PgConnection,
    project_id: i64,
    task_id: i64,
    plan: &TaskFileSetPlan,
) -> Result<TaskFileSetApplyResult, sqlx::Error> {
    let current = active_file_ids_tx(conn, project_id, task_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    let mut expected_current = plan.retained_file_ids.clone();
    expected_current.extend_from_slice(&plan.removed_file_ids);
    expected_current.sort_unstable();
    if current != expected_current {
        return Err(sqlx::Error::Protocol(
            "task file-set plan does not match locked current set".to_string(),
        ));
    }

    if !plan.removed_file_ids.is_empty() {
        let removed = sqlx::query(
            "DELETE FROM task_files
             WHERE task_id = $1 AND live_file_id = ANY($2::BIGINT[])",
        )
        .bind(task_id)
        .bind(&plan.removed_file_ids)
        .execute(&mut *conn)
        .await?;
        if removed.rows_affected() != plan.removed_file_ids.len() as u64 {
            return Err(sqlx::Error::Protocol(
                "task file removal precondition changed".to_string(),
            ));
        }
    }

    let mut baseline_entries_added = 0_i64;
    if !plan.added_file_ids.is_empty() {
        let eligible_files = sqlx::query_scalar::<_, i64>(
            "SELECT file.id FROM files AS file
             WHERE file.project_id = $1
               AND file.id = ANY($2::BIGINT[])
               AND file.deleted_at IS NULL
               AND NOT EXISTS (
                   SELECT 1 FROM folders AS ancestor
                   WHERE ancestor.project_id = file.project_id
                     AND ancestor.deleted_at IS NOT NULL
                     AND (file.path = ancestor.path OR file.path LIKE ancestor.path || '/%')
               )
             ORDER BY file.id FOR UPDATE",
        )
        .bind(project_id)
        .bind(&plan.added_file_ids)
        .fetch_all(&mut *conn)
        .await?;
        if eligible_files != plan.added_file_ids {
            return Err(sqlx::Error::RowNotFound);
        }

        for file_id in &plan.added_file_ids {
            let task_file_id: i64 = sqlx::query_scalar(
                "INSERT INTO task_files (task_id, file_id_snapshot, live_file_id)
                 VALUES ($1, $2, $2) RETURNING id",
            )
            .bind(task_id)
            .bind(file_id)
            .fetch_one(&mut *conn)
            .await?;
            // prts-core::tasks::include_in_baseline 的集合执行形态：
            // effectively_visible(..., false) AND state = untranslated。
            let inserted = sqlx::query(
                "INSERT INTO task_baseline_entries (
                     task_file_id, entry_id_snapshot, live_entry_id
                 )
                 SELECT $1, entry.id, entry.id
                 FROM entries AS entry
                 WHERE entry.file_id = $2
                   AND prts_entry_is_effectively_visible(entry)
                   AND entry.state = 'untranslated'
                 ORDER BY entry.id",
            )
            .bind(task_file_id)
            .bind(file_id)
            .execute(&mut *conn)
            .await?;
            baseline_entries_added += i64::try_from(inserted.rows_affected()).map_err(|_| {
                sqlx::Error::Protocol("task baseline count exceeds i64".to_string())
            })?;
        }
    }

    recompute_task_ids_tx(conn, &[task_id]).await?;
    Ok(TaskFileSetApplyResult {
        retained_files: plan.retained_file_ids.len(),
        added_files: plan.added_file_ids.len(),
        removed_files: plan.removed_file_ids.len(),
        baseline_entries_added,
    })
}

/// 按 typed progress 语义重算给定任务。只在 mutation 事务中调用，不用于正常读取。
pub async fn recompute_task_ids_tx(
    conn: &mut PgConnection,
    task_ids: &[i64],
) -> Result<(), sqlx::Error> {
    if task_ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "WITH target AS (
             SELECT DISTINCT unnest($1::BIGINT[]) AS task_id
         ), counts AS (
             SELECT target.task_id,
                    count(entry.id) FILTER (
                        WHERE prts_entry_is_effectively_visible(entry)
                    ) AS denominator,
                    count(entry.id) FILTER (
                        WHERE prts_entry_is_effectively_visible(entry)
                          AND entry.state <> 'untranslated'
                    ) AS completed
             FROM target
             LEFT JOIN task_files AS task_file ON task_file.task_id = target.task_id
             LEFT JOIN task_baseline_entries AS baseline
               ON baseline.task_file_id = task_file.id
             LEFT JOIN entries AS entry
               ON entry.id = baseline.live_entry_id
              AND entry.file_id = task_file.live_file_id
             GROUP BY target.task_id
         )
         INSERT INTO task_stats (task_id, denominator, completed, updated_at)
         SELECT task_id, denominator, completed, now() FROM counts
         ON CONFLICT (task_id) DO UPDATE SET
             denominator = EXCLUDED.denominator,
             completed = EXCLUDED.completed,
             updated_at = now()",
    )
    .bind(task_ids)
    .execute(conn)
    .await
    .map(|_| ())
}

/// 对引用单个 baseline entry 的任务应用 core typed before→after delta。
pub async fn apply_entry_transition_tx(
    conn: &mut PgConnection,
    entry_id: i64,
    before: TaskEntrySnapshot,
    after: TaskEntrySnapshot,
) -> Result<(), sqlx::Error> {
    let delta = prts_core::tasks::progress_delta(before, after);
    apply_entry_delta_tx(conn, entry_id, delta).await
}

async fn apply_entry_delta_tx(
    conn: &mut PgConnection,
    entry_id: i64,
    delta: TaskProgressDelta,
) -> Result<(), sqlx::Error> {
    if delta == TaskProgressDelta::default() {
        return Ok(());
    }
    sqlx::query(
        "UPDATE task_stats AS stats SET
             denominator = stats.denominator + $2,
             completed = stats.completed + $3,
             updated_at = now()
         FROM (
             SELECT DISTINCT task_file.task_id
             FROM task_baseline_entries AS baseline
             JOIN task_files AS task_file ON task_file.id = baseline.task_file_id
             WHERE baseline.live_entry_id = $1
         ) AS affected
         WHERE stats.task_id = affected.task_id",
    )
    .bind(entry_id)
    .bind(delta.denominator)
    .bind(delta.completed)
    .execute(conn)
    .await
    .map(|_| ())
}

/// 把数据库 wire state 转成 core 任务规则输入；非法值 fail closed。
pub fn entry_snapshot(
    state: &str,
    effectively_visible: bool,
) -> Result<TaskEntrySnapshot, sqlx::Error> {
    let state = EntryState::parse(state)
        .ok_or_else(|| sqlx::Error::Protocol("task entry state is invalid".to_string()))?;
    Ok(TaskEntrySnapshot {
        state,
        effectively_visible,
    })
}

/// 文件/文件夹 exposure 变化后重算引用这些 live files 的任务。
pub async fn recompute_for_file_ids_tx(
    conn: &mut PgConnection,
    file_ids: &[i64],
) -> Result<(), sqlx::Error> {
    if file_ids.is_empty() {
        return Ok(());
    }
    let task_ids = sqlx::query_scalar::<_, i64>(
        "SELECT DISTINCT task_id FROM task_files
         WHERE live_file_id = ANY($1::BIGINT[])
         ORDER BY task_id",
    )
    .bind(file_ids)
    .fetch_all(&mut *conn)
    .await?;
    recompute_task_ids_tx(conn, &task_ids).await
}

/// 低频结构 rollback/restore 后按项目重算任务，不进入读热路径。
pub async fn recompute_project_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<(), sqlx::Error> {
    let task_ids = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM tasks WHERE project_id = $1 ORDER BY id FOR UPDATE",
    )
    .bind(project_id)
    .fetch_all(&mut *conn)
    .await?;
    recompute_task_ids_tx(conn, &task_ids).await
}

/// file retention purge 的显式顺序：先 NULL live refs，再重算统计，之后调用方才删业务行。
pub async fn detach_live_refs_for_purge_tx(
    conn: &mut PgConnection,
    file_ids: &[i64],
    entry_ids: &[i64],
) -> Result<(), sqlx::Error> {
    let task_ids = sqlx::query_scalar::<_, i64>(
        "SELECT DISTINCT task_id FROM task_files
         WHERE live_file_id = ANY($1::BIGINT[])
         UNION
         SELECT DISTINCT task_file.task_id
         FROM task_baseline_entries AS baseline
         JOIN task_files AS task_file ON task_file.id = baseline.task_file_id
         WHERE baseline.live_entry_id = ANY($2::BIGINT[])
         ORDER BY 1",
    )
    .bind(file_ids)
    .bind(entry_ids)
    .fetch_all(&mut *conn)
    .await?;
    if !entry_ids.is_empty() {
        sqlx::query(
            "UPDATE task_baseline_entries SET live_entry_id = NULL
             WHERE live_entry_id = ANY($1::BIGINT[])",
        )
        .bind(entry_ids)
        .execute(&mut *conn)
        .await?;
    }
    if !file_ids.is_empty() {
        sqlx::query(
            "UPDATE task_files SET live_file_id = NULL
             WHERE live_file_id = ANY($1::BIGINT[])",
        )
        .bind(file_ids)
        .execute(&mut *conn)
        .await?;
    }
    recompute_task_ids_tx(conn, &task_ids).await
}

/// 阶段 6 current_task scope 的 DB 能力：只按当前 live task files 取当前可见词条，
/// 不联接 baseline；返回 None 表示 task/project 绑定失败。
pub async fn current_scope_entry_ids(
    pool: &PgPool,
    project_id: i64,
    task_id: i64,
    after: Option<i64>,
    limit: i64,
) -> Result<Option<Vec<i64>>, sqlx::Error> {
    if find(pool, project_id, task_id).await?.is_none() {
        return Ok(None);
    }
    sqlx::query_scalar(
        "SELECT entry.id
         FROM task_files AS task_file
         JOIN entries AS entry ON entry.file_id = task_file.live_file_id
         WHERE task_file.task_id = $1
           AND prts_entry_is_effectively_visible(entry)
           AND ($2::BIGINT IS NULL OR entry.id > $2)
         ORDER BY entry.id
         LIMIT $3",
    )
    .bind(task_id)
    .bind(after)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map(Some)
}
