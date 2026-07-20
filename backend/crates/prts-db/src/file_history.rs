//! 文件历史 typed plan 的 PostgreSQL adapter。
//!
//! 本模块只负责锁定/物化数据库状态并执行 `prts-core::file_history` 已决定的 mutation。
//! move/delete/restore/rollback 的路径、ownership、统计与 entry 状态真值不得在 SQL 中
//! 重新判断。

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use prts_core::file_history::{
    ActivePathIndex, FileHistoryEntity, FileHistoryItemOperation, FileHistoryMutation,
    FileHistoryOperation, FileHistoryPlan, FileHistorySnapshot, FileHistoryTarget, FileNode,
    FolderNode, MaterializedFileStats, MaterializedFileVersion, VersionedEntry,
};
use prts_core::upload_replacement::{EntryHistorySnapshot, OriginalText};
use prts_core::EntryState;
use serde_json::{json, Value};
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

/// 锁定查询中的 folder 行。
#[derive(Debug, FromRow)]
struct FolderNodeRow {
    id: i64,
    parent_id: Option<i64>,
    name: String,
    path: String,
    deletion_change_set_id: Option<Uuid>,
}

impl From<FolderNodeRow> for FolderNode {
    fn from(row: FolderNodeRow) -> Self {
        Self {
            id: row.id,
            parent_id: row.parent_id,
            name: row.name,
            path: row.path,
            deletion_operation_id: row.deletion_change_set_id,
        }
    }
}

/// 锁定查询中的 file + file_stats 行。
#[derive(Debug, FromRow)]
struct FileNodeRow {
    id: i64,
    folder_id: Option<i64>,
    name: String,
    path: String,
    deletion_change_set_id: Option<Uuid>,
    visible_total: i64,
    untranslated_count: i64,
    translated_count: i64,
    questioned_count: i64,
    checked_count: i64,
    reviewed_count: i64,
    hidden_total: i64,
    hidden_untranslated_count: i64,
    hidden_translated_count: i64,
    hidden_questioned_count: i64,
    hidden_checked_count: i64,
    hidden_reviewed_count: i64,
}

/// API 历史列表中的 change set 行。
#[derive(Debug, Clone, FromRow)]
pub struct FileChangeSetRecord {
    /// 稳定 UUID。
    pub id: Uuid,
    /// 所属项目。
    pub project_id: i64,
    /// 可空文件目标。
    pub file_id: Option<i64>,
    /// 可空文件夹目标。
    pub folder_id: Option<i64>,
    /// actor 删除后为 `None`。
    pub actor_id: Option<i64>,
    /// allowlisted 操作名。
    pub operation: String,
    /// 安全路径快照。
    pub path_snapshot: String,
    /// 无正文元数据。
    pub metadata: Value,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// API 历史列表中的 item 行。
#[derive(Debug, Clone, FromRow)]
pub struct FileChangeItemRecord {
    /// identity id。
    pub id: i64,
    /// 所属 change set。
    pub change_set_id: Uuid,
    /// folder/file/entry。
    pub entity_type: String,
    /// 永久实体 id snapshot。
    pub entity_id_snapshot: Option<i64>,
    /// allowlisted item operation。
    pub operation: String,
    /// 变更前 allowlisted JSON。
    pub before_value: Option<Value>,
    /// 变更后 allowlisted JSON。
    pub after_value: Option<Value>,
    /// change set 内顺序。
    pub ordinal: i32,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 历史列表的一项及其 deltas。
#[derive(Debug, Clone)]
pub struct FileHistoryRecord {
    /// change set。
    pub change_set: FileChangeSetRecord,
    /// 按 ordinal 排序的 items。
    pub items: Vec<FileChangeItemRecord>,
}

/// 到期删除 operation 的键集扫描项。
#[derive(Debug, Clone, FromRow)]
pub struct DueDeletionOperation {
    /// 原删除 change set。
    pub change_set_id: Uuid,
    /// 项目 snapshot。
    pub project_id: i64,
    /// 直接文件目标。
    pub file_id: Option<i64>,
    /// 文件夹树目标。
    pub folder_id: Option<i64>,
    /// 到期时间。
    pub purge_after: DateTime<Utc>,
}

/// 永久清除后写入审计的无正文摘要。
#[derive(Debug, Clone)]
pub struct PurgedFileTree {
    /// 项目 snapshot。
    pub project_id: i64,
    /// 根目标。
    pub target: FileHistoryTarget,
    /// 原删除 change set。
    pub deletion_change_set_id: Uuid,
    /// 安全路径 snapshot。
    pub path: String,
    /// 清除 folder 数。
    pub folder_count: usize,
    /// 清除 file 数。
    pub file_count: usize,
    /// 清除 entry 数。
    pub entry_count: usize,
}

impl From<FileNodeRow> for FileNode {
    fn from(row: FileNodeRow) -> Self {
        Self {
            id: row.id,
            folder_id: row.folder_id,
            name: row.name,
            path: row.path,
            deletion_operation_id: row.deletion_change_set_id,
            stats: MaterializedFileStats {
                visible_total: row.visible_total,
                untranslated: row.untranslated_count,
                translated: row.translated_count,
                questioned: row.questioned_count,
                checked: row.checked_count,
                reviewed: row.reviewed_count,
                hidden_total: row.hidden_total,
                hidden_untranslated: row.hidden_untranslated_count,
                hidden_translated: row.hidden_translated_count,
                hidden_questioned: row.hidden_questioned_count,
                hidden_checked: row.hidden_checked_count,
                hidden_reviewed: row.hidden_reviewed_count,
            },
        }
    }
}

/// 在项目事务内锁定一个文件（active/deleted 均可）。
pub async fn lock_file_tx(
    conn: &mut PgConnection,
    project_id: i64,
    file_id: i64,
) -> Result<Option<FileNode>, sqlx::Error> {
    sqlx::query_as::<_, FileNodeRow>(
        "SELECT file.id, file.folder_id, file.name, file.path,
                file.deletion_change_set_id,
                COALESCE(stats.visible_total, 0)::BIGINT AS visible_total,
                COALESCE(stats.untranslated_count, 0)::BIGINT AS untranslated_count,
                COALESCE(stats.translated_count, 0)::BIGINT AS translated_count,
                COALESCE(stats.questioned_count, 0)::BIGINT AS questioned_count,
                COALESCE(stats.checked_count, 0)::BIGINT AS checked_count,
                COALESCE(stats.reviewed_count, 0)::BIGINT AS reviewed_count,
                COALESCE(stats.hidden_total, 0)::BIGINT AS hidden_total,
                COALESCE(stats.hidden_untranslated_count, 0)::BIGINT
                    AS hidden_untranslated_count,
                COALESCE(stats.hidden_translated_count, 0)::BIGINT
                    AS hidden_translated_count,
                COALESCE(stats.hidden_questioned_count, 0)::BIGINT
                    AS hidden_questioned_count,
                COALESCE(stats.hidden_checked_count, 0)::BIGINT AS hidden_checked_count,
                COALESCE(stats.hidden_reviewed_count, 0)::BIGINT AS hidden_reviewed_count
         FROM files AS file
         LEFT JOIN file_stats AS stats ON stats.file_id = file.id
         WHERE file.project_id = $1 AND file.id = $2
         FOR UPDATE OF file",
    )
    .bind(project_id)
    .bind(file_id)
    .fetch_optional(conn)
    .await
    .map(|row| row.map(Into::into))
}

/// 在项目事务内锁定一个文件夹（active/deleted 均可）。
pub async fn lock_folder_tx(
    conn: &mut PgConnection,
    project_id: i64,
    folder_id: i64,
) -> Result<Option<FolderNode>, sqlx::Error> {
    sqlx::query_as::<_, FolderNodeRow>(
        "SELECT id, parent_id, name, path, deletion_change_set_id
         FROM folders WHERE project_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(project_id)
    .bind(folder_id)
    .fetch_optional(conn)
    .await
    .map(|row| row.map(Into::into))
}

/// 锁定项目全部文件夹，供 core 计算 ancestor deletion exposure。
pub async fn lock_all_folders_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<Vec<FolderNode>, sqlx::Error> {
    sqlx::query_as::<_, FolderNodeRow>(
        "SELECT id, parent_id, name, path, deletion_change_set_id
         FROM folders WHERE project_id = $1 ORDER BY id FOR UPDATE",
    )
    .bind(project_id)
    .fetch_all(conn)
    .await
    .map(|rows| rows.into_iter().map(Into::into).collect())
}

/// 锁定一个 folder subtree 的全部 folder/file 行。
pub async fn lock_folder_subtree_tx(
    conn: &mut PgConnection,
    project_id: i64,
    root_id: i64,
) -> Result<(Vec<FolderNode>, Vec<FileNode>), sqlx::Error> {
    let folders: Vec<FolderNode> = sqlx::query_as::<_, FolderNodeRow>(
        "WITH RECURSIVE tree AS (
             SELECT id FROM folders WHERE project_id = $1 AND id = $2
             UNION ALL
             SELECT child.id FROM folders AS child
             JOIN tree ON child.parent_id = tree.id
             WHERE child.project_id = $1
         )
         SELECT folder.id, folder.parent_id, folder.name, folder.path,
                folder.deletion_change_set_id
         FROM folders AS folder JOIN tree ON tree.id = folder.id
         ORDER BY folder.id FOR UPDATE OF folder",
    )
    .bind(project_id)
    .bind(root_id)
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(Into::into)
    .collect::<Vec<_>>();
    let folder_ids = folders.iter().map(|folder| folder.id).collect::<Vec<_>>();
    let files = sqlx::query_as::<_, FileNodeRow>(
        "SELECT file.id, file.folder_id, file.name, file.path,
                file.deletion_change_set_id,
                COALESCE(stats.visible_total, 0)::BIGINT AS visible_total,
                COALESCE(stats.untranslated_count, 0)::BIGINT AS untranslated_count,
                COALESCE(stats.translated_count, 0)::BIGINT AS translated_count,
                COALESCE(stats.questioned_count, 0)::BIGINT AS questioned_count,
                COALESCE(stats.checked_count, 0)::BIGINT AS checked_count,
                COALESCE(stats.reviewed_count, 0)::BIGINT AS reviewed_count,
                COALESCE(stats.hidden_total, 0)::BIGINT AS hidden_total,
                COALESCE(stats.hidden_untranslated_count, 0)::BIGINT
                    AS hidden_untranslated_count,
                COALESCE(stats.hidden_translated_count, 0)::BIGINT
                    AS hidden_translated_count,
                COALESCE(stats.hidden_questioned_count, 0)::BIGINT
                    AS hidden_questioned_count,
                COALESCE(stats.hidden_checked_count, 0)::BIGINT AS hidden_checked_count,
                COALESCE(stats.hidden_reviewed_count, 0)::BIGINT AS hidden_reviewed_count
         FROM files AS file
         LEFT JOIN file_stats AS stats ON stats.file_id = file.id
         WHERE file.project_id = $1
           AND file.folder_id = ANY($2::BIGINT[])
         ORDER BY file.id FOR UPDATE OF file",
    )
    .bind(project_id)
    .bind(&folder_ids)
    .fetch_all(conn)
    .await?
    .into_iter()
    .map(Into::into)
    .collect();
    Ok((folders, files))
}

/// 读取 active path 集合；项目行锁保证随后 writer 与该快照串行。
pub async fn active_paths_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<ActivePathIndex, sqlx::Error> {
    let folders = sqlx::query_scalar::<_, String>(
        "SELECT path FROM folders WHERE project_id = $1 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .collect::<BTreeSet<_>>();
    let files = sqlx::query_scalar::<_, String>(
        "SELECT path FROM files WHERE project_id = $1 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_all(conn)
    .await?
    .into_iter()
    .collect::<BTreeSet<_>>();
    Ok(ActivePathIndex { folders, files })
}

/// 创建 active 文件夹；名称/child path 必须已由 core 校验。
pub async fn create_folder_tx(
    conn: &mut PgConnection,
    project_id: i64,
    parent_id: Option<i64>,
    name: &str,
    path: &str,
) -> Result<crate::models::Folder, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO folders (project_id, parent_id, name, path)
         VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(project_id)
    .bind(parent_id)
    .bind(name)
    .bind(path)
    .fetch_one(conn)
    .await
}

/// 以 `(created_at DESC, id DESC)` 键集列出项目历史。
pub async fn list_history(
    pool: &PgPool,
    project_id: i64,
    after: Option<Uuid>,
    file_id: Option<i64>,
    folder_id: Option<i64>,
    limit: i64,
) -> Result<Vec<FileHistoryRecord>, sqlx::Error> {
    let sets = sqlx::query_as::<_, FileChangeSetRecord>(
        "SELECT change_set.*
         FROM file_change_sets AS change_set
         WHERE change_set.project_id = $1
           AND ($2::BIGINT IS NULL OR change_set.file_id = $2)
           AND ($3::BIGINT IS NULL OR change_set.folder_id = $3)
           AND (
               $4::UUID IS NULL
               OR (change_set.created_at, change_set.id) < (
                   SELECT cursor.created_at, cursor.id
                   FROM file_change_sets AS cursor
                   WHERE cursor.project_id = $1 AND cursor.id = $4
               )
           )
         ORDER BY change_set.created_at DESC, change_set.id DESC
         LIMIT $5",
    )
    .bind(project_id)
    .bind(file_id)
    .bind(folder_id)
    .bind(after)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    if sets.is_empty() {
        return Ok(Vec::new());
    }
    let ids = sets.iter().map(|set| set.id).collect::<Vec<_>>();
    let items = sqlx::query_as::<_, FileChangeItemRecord>(
        "SELECT id, change_set_id, entity_type, entity_id_snapshot, operation,
                before_value, after_value, ordinal, created_at
         FROM file_change_items
         WHERE change_set_id = ANY($1::UUID[])
         ORDER BY change_set_id, ordinal",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let mut by_set = items.into_iter().fold(
        HashMap::<Uuid, Vec<FileChangeItemRecord>>::new(),
        |mut grouped, item| {
            grouped.entry(item.change_set_id).or_default().push(item);
            grouped
        },
    );
    Ok(sets
        .into_iter()
        .map(|change_set| FileHistoryRecord {
            items: by_set.remove(&change_set.id).unwrap_or_default(),
            change_set,
        })
        .collect())
}

/// 键集扫描仍由原 operation 持有且已到期的删除根。
pub async fn list_due_deletions(
    pool: &PgPool,
    after: Option<(DateTime<Utc>, Uuid)>,
    limit: i64,
) -> Result<Vec<DueDeletionOperation>, sqlx::Error> {
    let (after_time, after_id) = after.unzip();
    sqlx::query_as(
        "SELECT change_set.id AS change_set_id, change_set.project_id,
                change_set.file_id, change_set.folder_id,
                COALESCE(folder.purge_after, file.purge_after) AS purge_after
         FROM file_change_sets AS change_set
         LEFT JOIN folders AS folder
           ON folder.id = change_set.folder_id
          AND folder.deletion_change_set_id = change_set.id
         LEFT JOIN files AS file
           ON file.id = change_set.file_id
          AND file.deletion_change_set_id = change_set.id
         WHERE change_set.operation = 'delete'
           AND COALESCE(folder.purge_after, file.purge_after) <= now()
           AND (
               $1::TIMESTAMPTZ IS NULL
               OR (COALESCE(folder.purge_after, file.purge_after), change_set.id) > ($1, $2)
           )
         ORDER BY COALESCE(folder.purge_after, file.purge_after), change_set.id
         LIMIT $3",
    )
    .bind(after_time)
    .bind(after_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 按固定 FK 顺序永久清除一个到期树；operation 已恢复/尚未到期时返回 `None`。
pub async fn purge_due_operation_tx(
    conn: &mut PgConnection,
    operation: &DueDeletionOperation,
    now: DateTime<Utc>,
) -> Result<Option<PurgedFileTree>, sqlx::Error> {
    let project_locked: Option<i64> =
        sqlx::query_scalar("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
            .bind(operation.project_id)
            .fetch_optional(&mut *conn)
            .await?;
    if project_locked.is_none() {
        return Ok(None);
    }
    let locked: Option<(String, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT path_snapshot, file_id, folder_id
         FROM file_change_sets
         WHERE id = $1 AND project_id = $2 AND operation = 'delete'
         FOR UPDATE",
    )
    .bind(operation.change_set_id)
    .bind(operation.project_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((path_snapshot, file_id, folder_id)) = locked else {
        return Ok(None);
    };

    let (folder_ids, file_ids, target) = if let Some(root_folder_id) = folder_id {
        let root: Option<(String, Option<Uuid>, Option<DateTime<Utc>>)> = sqlx::query_as(
            "SELECT path, deletion_change_set_id, purge_after
             FROM folders WHERE id = $1 AND project_id = $2 FOR UPDATE",
        )
        .bind(root_folder_id)
        .bind(operation.project_id)
        .fetch_optional(&mut *conn)
        .await?;
        let Some((_root_path, owner, root_purge_after)) = root else {
            return Ok(None);
        };
        if owner != Some(operation.change_set_id) || root_purge_after.is_none_or(|due| due > now) {
            return Ok(None);
        }
        let folders = sqlx::query_as::<_, (i64, String, Option<DateTime<Utc>>)>(
            "WITH RECURSIVE tree AS (
                 SELECT id FROM folders WHERE project_id = $1 AND id = $2
                 UNION ALL
                 SELECT child.id FROM folders AS child
                 JOIN tree ON child.parent_id = tree.id
                 WHERE child.project_id = $1
             )
             SELECT folder.id, folder.path, folder.purge_after
             FROM folders AS folder JOIN tree ON tree.id = folder.id
             ORDER BY char_length(folder.path) DESC, folder.id DESC
             FOR UPDATE OF folder",
        )
        .bind(operation.project_id)
        .bind(root_folder_id)
        .fetch_all(&mut *conn)
        .await?;
        let subtree_folder_ids = folders.iter().map(|(id, _, _)| *id).collect::<Vec<_>>();
        let files = sqlx::query_as::<_, (i64, Option<DateTime<Utc>>)>(
            "SELECT id, purge_after FROM files
             WHERE project_id = $1
               AND folder_id = ANY($2::BIGINT[])
             ORDER BY id FOR UPDATE",
        )
        .bind(operation.project_id)
        .bind(&subtree_folder_ids)
        .fetch_all(&mut *conn)
        .await?;
        if folders
            .iter()
            .any(|(_, _, due)| due.is_none_or(|due| due > now))
            || files.iter().any(|(_, due)| due.is_none_or(|due| due > now))
        {
            return Ok(None);
        }
        (
            folders
                .into_iter()
                .map(|(id, path, _)| (id, path))
                .collect::<Vec<_>>(),
            files.into_iter().map(|(id, _)| id).collect::<Vec<_>>(),
            FileHistoryTarget::Folder(root_folder_id),
        )
    } else if let Some(target_file_id) = file_id {
        let file: Option<(Option<Uuid>, Option<DateTime<Utc>>)> = sqlx::query_as(
            "SELECT deletion_change_set_id, purge_after FROM files
             WHERE id = $1 AND project_id = $2 FOR UPDATE",
        )
        .bind(target_file_id)
        .bind(operation.project_id)
        .fetch_optional(&mut *conn)
        .await?;
        let Some((owner, due)) = file else {
            return Ok(None);
        };
        if owner != Some(operation.change_set_id) || due.is_none_or(|due| due > now) {
            return Ok(None);
        }
        (
            Vec::new(),
            vec![target_file_id],
            FileHistoryTarget::File(target_file_id),
        )
    } else {
        return Err(sqlx::Error::Protocol(
            "delete change set has no target".to_string(),
        ));
    };

    let entry_ids = if file_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_scalar::<_, i64>(
            "SELECT id FROM entries WHERE file_id = ANY($1::BIGINT[]) ORDER BY id FOR UPDATE",
        )
        .bind(&file_ids)
        .fetch_all(&mut *conn)
        .await?
    };
    let folder_id_values = folder_ids.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    let related_change_sets = sqlx::query_scalar::<_, Uuid>(
        "SELECT change_set.id
         FROM file_change_sets AS change_set
         WHERE change_set.project_id = $1
           AND change_set.id IN (
               SELECT candidate.id
               FROM file_change_sets AS candidate
               LEFT JOIN file_change_items AS item ON item.change_set_id = candidate.id
               WHERE candidate.project_id = $1
                 AND (
                     candidate.file_id = ANY($2::BIGINT[])
                     OR candidate.folder_id = ANY($3::BIGINT[])
                     OR (item.entity_type = 'file'
                         AND item.entity_id_snapshot = ANY($2::BIGINT[]))
                     OR (item.entity_type = 'folder'
                         AND item.entity_id_snapshot = ANY($3::BIGINT[]))
                     OR (item.entity_type = 'entry'
                         AND item.entity_id_snapshot = ANY($4::BIGINT[]))
                     OR candidate.id = $5
                 )
           )
         FOR UPDATE",
    )
    .bind(operation.project_id)
    .bind(&file_ids)
    .bind(&folder_id_values)
    .bind(&entry_ids)
    .bind(operation.change_set_id)
    .fetch_all(&mut *conn)
    .await?;
    // `0010` 同时声明 target SET NULL 与“非 upload 必须有 target”的即时 CHECK。
    // 冻结迁移不可修改，因此 purge 事务用一个绝不提交的内部 anchor 暂时承接 target。
    let purge_anchor_id = if related_change_sets.is_empty() {
        None
    } else {
        let anchor_name = format!("__prts_internal_purge_anchor_{}", Uuid::new_v4().simple());
        let anchor_id: i64 = sqlx::query_scalar(
            "INSERT INTO folders (project_id, parent_id, name, path)
             VALUES ($1, NULL, $2, $2) RETURNING id",
        )
        .bind(operation.project_id)
        .bind(&anchor_name)
        .fetch_one(&mut *conn)
        .await?;
        sqlx::query(
            "UPDATE file_change_sets SET file_id = NULL, folder_id = $2
             WHERE id = ANY($1::UUID[])",
        )
        .bind(&related_change_sets)
        .bind(anchor_id)
        .execute(&mut *conn)
        .await?;
        Some(anchor_id)
    };

    // 跨域 live refs 先显式 detach；immutable snapshot IDs 与 audit 不动。
    if !file_ids.is_empty() {
        sqlx::query(
            "UPDATE jobs SET
                 target_file_id = NULL,
                 state = CASE WHEN state IN ('queued', 'running', 'paused')
                              THEN 'cancelled' ELSE state END,
                 worker_id = CASE WHEN state IN ('queued', 'running', 'paused')
                                  THEN NULL ELSE worker_id END,
                 lease_until = CASE WHEN state IN ('queued', 'running', 'paused')
                                    THEN NULL ELSE lease_until END,
                 finished_at = CASE WHEN state IN ('queued', 'running', 'paused')
                                    THEN now() ELSE finished_at END,
                 updated_at = now()
             WHERE target_file_id = ANY($1::BIGINT[])",
        )
        .bind(&file_ids)
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "UPDATE upload_batch_files SET target_file_id = NULL
             WHERE target_file_id = ANY($1::BIGINT[])",
        )
        .bind(&file_ids)
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "UPDATE upload_file_attempts SET target_file_id = NULL
             WHERE target_file_id = ANY($1::BIGINT[])",
        )
        .bind(&file_ids)
        .execute(&mut *conn)
        .await?;
    }
    // task live refs 必须在删除 entries/files 之前显式置 NULL；immutable snapshot IDs 保留。
    super::tasks::detach_live_refs_for_purge_tx(conn, &file_ids, &entry_ids).await?;
    if !entry_ids.is_empty() {
        sqlx::query(
            "UPDATE language_resolution_issues SET entry_id = NULL
             WHERE entry_id = ANY($1::BIGINT[])",
        )
        .bind(&entry_ids)
        .execute(&mut *conn)
        .await?;
        // entry-derived search/vector 当前与 entries 同行；显式清 versions 后再删业务行。
        sqlx::query("DELETE FROM entry_versions WHERE entry_id = ANY($1::BIGINT[])")
            .bind(&entry_ids)
            .execute(&mut *conn)
            .await?;
        sqlx::query("DELETE FROM entries WHERE id = ANY($1::BIGINT[])")
            .bind(&entry_ids)
            .execute(&mut *conn)
            .await?;
    }
    if !file_ids.is_empty() {
        sqlx::query("DELETE FROM files WHERE id = ANY($1::BIGINT[])")
            .bind(&file_ids)
            .execute(&mut *conn)
            .await?;
    }
    for (folder_id, _) in &folder_ids {
        let deleted = sqlx::query("DELETE FROM folders WHERE id = $1")
            .bind(folder_id)
            .execute(&mut *conn)
            .await?;
        require_affected("purge folder leaf", deleted.rows_affected(), 1)?;
    }

    if !related_change_sets.is_empty() {
        sqlx::query("DELETE FROM file_change_items WHERE change_set_id = ANY($1::UUID[])")
            .bind(&related_change_sets)
            .execute(&mut *conn)
            .await?;
        sqlx::query("DELETE FROM file_change_sets WHERE id = ANY($1::UUID[])")
            .bind(&related_change_sets)
            .execute(&mut *conn)
            .await?;
    }
    if let Some(anchor_id) = purge_anchor_id {
        let deleted = sqlx::query("DELETE FROM folders WHERE id = $1")
            .bind(anchor_id)
            .execute(conn)
            .await?;
        require_affected("delete purge anchor", deleted.rows_affected(), 1)?;
    }
    Ok(Some(PurgedFileTree {
        project_id: operation.project_id,
        target,
        deletion_change_set_id: operation.change_set_id,
        path: path_snapshot,
        folder_count: folder_ids.len(),
        file_count: file_ids.len(),
        entry_count: entry_ids.len(),
    }))
}

/// 锁定 current file/entries，并通过逆放 target 之后的 file-change deltas 物化目标版本。
///
/// 普通 entry 编辑仍由 entry history 管理；文件 rollback 只逆放 file history writer
/// 记录的结构/replacement/restore/tombstone delta，不把两套历史语义混在 SQL 中。
pub async fn materialize_file_rollback_tx(
    conn: &mut PgConnection,
    project_id: i64,
    file_id: i64,
    target_change_set_id: Uuid,
) -> Result<(MaterializedFileVersion, MaterializedFileVersion), sqlx::Error> {
    let current_file = lock_file_tx(&mut *conn, project_id, file_id)
        .await?
        .ok_or_else(|| sqlx::Error::RowNotFound)?;
    let target_set: FileChangeSetRecord = sqlx::query_as(
        "SELECT * FROM file_change_sets
         WHERE project_id = $1 AND id = $2
           AND (
               file_id = $3
               OR EXISTS (
                   SELECT 1 FROM file_change_items AS item
                   WHERE item.change_set_id = file_change_sets.id
                     AND item.entity_type = 'file'
                     AND item.entity_id_snapshot = $3
               )
           )
         FOR UPDATE",
    )
    .bind(project_id)
    .bind(target_change_set_id)
    .bind(file_id)
    .fetch_one(&mut *conn)
    .await?;

    let current_entries = sqlx::query_as::<_, CurrentEntryRow>(
        "SELECT id, key, original, translation, state, locked, hidden, deleted_at
         FROM entries WHERE project_id = $1 AND file_id = $2 ORDER BY id FOR UPDATE",
    )
    .bind(project_id)
    .bind(file_id)
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(CurrentEntryRow::into_versioned)
    .collect::<Result<Vec<_>, _>>()?;
    let current = MaterializedFileVersion {
        file: current_file.clone(),
        entries: current_entries.clone(),
    };
    let mut target_file = current_file;
    let mut target_entries = current_entries
        .into_iter()
        .map(|entry| (entry.id, entry.snapshot))
        .collect::<HashMap<_, _>>();

    let later_items = sqlx::query_as::<_, ReverseHistoryRow>(
        "SELECT item.entity_type, item.entity_id_snapshot, item.before_value
         FROM file_change_sets AS change_set
         JOIN file_change_items AS item ON item.change_set_id = change_set.id
         WHERE change_set.project_id = $1
           AND (change_set.created_at, change_set.id) > ($3, $4)
           AND (
               (item.entity_type = 'file' AND item.entity_id_snapshot = $2)
               OR (
                   item.entity_type = 'entry'
                   AND EXISTS (
                       SELECT 1 FROM entries AS entry
                       WHERE entry.id = item.entity_id_snapshot AND entry.file_id = $2
                   )
               )
           )
         ORDER BY change_set.created_at DESC, change_set.id DESC, item.ordinal DESC",
    )
    .bind(project_id)
    .bind(file_id)
    .bind(target_set.created_at)
    .bind(target_set.id)
    .fetch_all(conn)
    .await?;
    for item in later_items {
        let Some(entity_id) = item.entity_id_snapshot else {
            continue;
        };
        match item.entity_type.as_str() {
            "file" => {
                if let Some(before) = item.before_value {
                    apply_file_snapshot(&mut target_file, before)?;
                }
            }
            "entry" => match item.before_value {
                Some(before) => {
                    target_entries.insert(entity_id, decode_entry_snapshot(before)?);
                }
                None => {
                    target_entries.remove(&entity_id);
                }
            },
            _ => {}
        }
    }
    let mut entries = target_entries
        .into_iter()
        .map(|(id, snapshot)| VersionedEntry { id, snapshot })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.id);
    Ok((
        current,
        MaterializedFileVersion {
            file: target_file,
            entries,
        },
    ))
}

/// 物化 folder root 在目标 change set 之后的结构版本，并返回当前完整 subtree。
pub async fn materialize_folder_rollback_tx(
    conn: &mut PgConnection,
    project_id: i64,
    folder_id: i64,
    target_change_set_id: Uuid,
) -> Result<(FolderNode, Vec<FolderNode>, Vec<FileNode>, FolderNode), sqlx::Error> {
    let current_root = lock_folder_tx(&mut *conn, project_id, folder_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    let target_set: FileChangeSetRecord = sqlx::query_as(
        "SELECT * FROM file_change_sets
         WHERE project_id = $1 AND id = $2
           AND (
               folder_id = $3
               OR EXISTS (
                   SELECT 1 FROM file_change_items AS item
                   WHERE item.change_set_id = file_change_sets.id
                     AND item.entity_type = 'folder'
                     AND item.entity_id_snapshot = $3
               )
           )
         FOR UPDATE",
    )
    .bind(project_id)
    .bind(target_change_set_id)
    .bind(folder_id)
    .fetch_one(&mut *conn)
    .await?;
    let (folders, files) = lock_folder_subtree_tx(&mut *conn, project_id, current_root.id).await?;
    let descendants = folders
        .into_iter()
        .filter(|folder| folder.id != folder_id)
        .collect::<Vec<_>>();
    let mut target_root = current_root.clone();
    let later = sqlx::query_scalar::<_, Option<Value>>(
        "SELECT item.before_value
         FROM file_change_sets AS change_set
         JOIN file_change_items AS item ON item.change_set_id = change_set.id
         WHERE change_set.project_id = $1
           AND (change_set.created_at, change_set.id) > ($3, $4)
           AND item.entity_type = 'folder'
           AND item.entity_id_snapshot = $2
         ORDER BY change_set.created_at DESC, change_set.id DESC, item.ordinal DESC",
    )
    .bind(project_id)
    .bind(folder_id)
    .bind(target_set.created_at)
    .bind(target_set.id)
    .fetch_all(conn)
    .await?;
    for before in later.into_iter().flatten() {
        apply_folder_snapshot(&mut target_root, before)?;
    }
    Ok((current_root, descendants, files, target_root))
}

#[derive(Debug, FromRow)]
struct CurrentEntryRow {
    id: i64,
    key: String,
    original: Value,
    translation: String,
    state: String,
    locked: bool,
    hidden: bool,
    deleted_at: Option<DateTime<Utc>>,
}

impl CurrentEntryRow {
    fn into_versioned(self) -> Result<VersionedEntry, sqlx::Error> {
        Ok(VersionedEntry {
            id: self.id,
            snapshot: EntryHistorySnapshot {
                key: self.key,
                original: serde_json::from_value::<OriginalText>(self.original).map_err(|_| {
                    sqlx::Error::Protocol("file history original snapshot is invalid".to_string())
                })?,
                translation: self.translation,
                state: EntryState::parse(&self.state).ok_or_else(|| {
                    sqlx::Error::Protocol("file history entry state is invalid".to_string())
                })?,
                locked: self.locked,
                hidden: self.hidden,
                deleted: self.deleted_at.is_some(),
            },
        })
    }
}

#[derive(Debug, FromRow)]
struct ReverseHistoryRow {
    entity_type: String,
    entity_id_snapshot: Option<i64>,
    before_value: Option<Value>,
}

fn apply_file_snapshot(file: &mut FileNode, value: Value) -> Result<(), sqlx::Error> {
    let object = value.as_object().ok_or_else(|| {
        sqlx::Error::Protocol("file history file snapshot is not an object".to_string())
    })?;
    file.folder_id = object.get("folder_id").and_then(Value::as_i64);
    file.name = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| sqlx::Error::Protocol("file history name is missing".to_string()))?
        .to_string();
    file.path = object
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| sqlx::Error::Protocol("file history path is missing".to_string()))?
        .to_string();
    file.deletion_operation_id = if object
        .get("deleted_at")
        .is_some_and(|value| !value.is_null())
    {
        Some(Uuid::nil())
    } else {
        None
    };
    Ok(())
}

fn apply_folder_snapshot(folder: &mut FolderNode, value: Value) -> Result<(), sqlx::Error> {
    let object = value.as_object().ok_or_else(|| {
        sqlx::Error::Protocol("file history folder snapshot is not an object".to_string())
    })?;
    folder.parent_id = object.get("parent_id").and_then(Value::as_i64);
    folder.name = json_string(object, "name")?;
    folder.path = json_string(object, "path")?;
    folder.deletion_operation_id = if object
        .get("deleted_at")
        .is_some_and(|value| !value.is_null())
    {
        Some(Uuid::nil())
    } else {
        None
    };
    Ok(())
}

fn decode_entry_snapshot(value: Value) -> Result<EntryHistorySnapshot, sqlx::Error> {
    let object = value.as_object().ok_or_else(|| {
        sqlx::Error::Protocol("file history entry snapshot is not an object".to_string())
    })?;
    Ok(EntryHistorySnapshot {
        key: json_string(object, "key")?,
        original: serde_json::from_value::<OriginalText>(
            object.get("original").cloned().ok_or_else(|| {
                sqlx::Error::Protocol("file history original is missing".to_string())
            })?,
        )
        .map_err(|_| sqlx::Error::Protocol("file history original is invalid".to_string()))?,
        translation: json_string(object, "translation")?,
        state: EntryState::parse(&json_string(object, "state")?)
            .ok_or_else(|| sqlx::Error::Protocol("file history state is invalid".to_string()))?,
        locked: object
            .get("locked")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                sqlx::Error::Protocol("file history locked flag is missing".to_string())
            })?,
        hidden: object
            .get("hidden")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                sqlx::Error::Protocol("file history hidden flag is missing".to_string())
            })?,
        deleted: object
            .get("deleted_at")
            .is_some_and(|value| !value.is_null()),
    })
}

fn json_string(
    object: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<String, sqlx::Error> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| sqlx::Error::Protocol(format!("file history {field} is missing")))
}

/// 执行 core 已生成的完整 history plan。
#[allow(clippy::too_many_arguments)]
pub async fn apply_plan_tx(
    conn: &mut PgConnection,
    project_id: i64,
    actor_id: i64,
    effective_at: DateTime<Utc>,
    purge_after: DateTime<Utc>,
    plan: &FileHistoryPlan,
) -> Result<(), sqlx::Error> {
    let prepared_history = prepare_history_rows(conn, plan, effective_at).await?;
    let (file_id, folder_id) = match plan.target {
        FileHistoryTarget::File(id) => (Some(id), None),
        FileHistoryTarget::Folder(id) => (None, Some(id)),
    };
    let operation = operation_name(plan.operation);
    let metadata = json!({
        "source_change_set_id": plan.source_change_set_id,
        "cp_delta_tenths": plan.cp_delta_tenths,
        "stats_delta": stats_json(plan.project_stats_delta),
    });
    sqlx::query(
        "INSERT INTO file_change_sets (
             id, project_id, file_id, folder_id, actor_id, operation,
             path_snapshot, metadata, created_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(plan.change_set_id)
    .bind(project_id)
    .bind(file_id)
    .bind(folder_id)
    .bind(actor_id)
    .bind(operation)
    .bind(&plan.path_snapshot)
    .bind(metadata)
    .bind(effective_at)
    .execute(&mut *conn)
    .await?;

    let suppress_entry_stats = plan
        .mutations
        .iter()
        .any(|mutation| matches!(mutation, FileHistoryMutation::ReplaceEntry { .. }));
    let suppressed_file_id = if suppress_entry_stats {
        Some(file_id.ok_or_else(|| {
            sqlx::Error::Protocol("entry rollback plan must target a file".to_string())
        })?)
    } else {
        None
    };
    if let Some(target_file_id) = suppressed_file_id {
        let hidden = sqlx::query(
            "UPDATE files
             SET deleted_at = $3, deleted_by = $4, purge_after = $3,
                 deletion_change_set_id = $5
             WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(project_id)
        .bind(target_file_id)
        .bind(effective_at)
        .bind(actor_id)
        .bind(plan.change_set_id)
        .execute(&mut *conn)
        .await?;
        require_affected("hide rollback file", hidden.rows_affected(), 1)?;
    }

    for mutation in &plan.mutations {
        apply_mutation_tx(
            &mut *conn,
            project_id,
            actor_id,
            effective_at,
            purge_after,
            plan.change_set_id,
            mutation,
        )
        .await?;
    }

    apply_stats_delta_tx(
        &mut *conn,
        project_id,
        plan.project_stats_delta,
        plan.file_stats_delta,
    )
    .await?;

    for row in prepared_history {
        sqlx::query(
            "INSERT INTO file_change_items (
                 change_set_id, entity_type, entity_id_snapshot, operation,
                 before_value, after_value, ordinal
             ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(plan.change_set_id)
        .bind(row.entity_type)
        .bind(row.entity_id)
        .bind(row.operation)
        .bind(row.before_value)
        .bind(row.after_value)
        .bind(row.ordinal)
        .execute(&mut *conn)
        .await?;
    }

    if let Some(target_file_id) = suppressed_file_id {
        let restored = sqlx::query(
            "UPDATE files
             SET deleted_at = NULL, deleted_by = NULL, purge_after = NULL,
                 deletion_change_set_id = NULL
             WHERE project_id = $1 AND id = $2 AND deletion_change_set_id = $3",
        )
        .bind(project_id)
        .bind(target_file_id)
        .bind(plan.change_set_id)
        .execute(&mut *conn)
        .await?;
        require_affected("restore rollback file", restored.rows_affected(), 1)?;
    }
    verify_stats_nonnegative_tx(conn, project_id, plan.file_stats_delta.map(|value| value.0))
        .await?;
    super::tasks::recompute_project_tx(conn, project_id).await
}

async fn apply_mutation_tx(
    conn: &mut PgConnection,
    project_id: i64,
    actor_id: i64,
    effective_at: DateTime<Utc>,
    purge_after: DateTime<Utc>,
    change_set_id: Uuid,
    mutation: &FileHistoryMutation,
) -> Result<(), sqlx::Error> {
    match mutation {
        FileHistoryMutation::UpdateFolderStructure { before, after } => {
            let result = sqlx::query(
                "UPDATE folders SET parent_id = $3, name = $4, path = $5
                 WHERE project_id = $1 AND id = $2 AND path = $6
                   AND deletion_change_set_id IS NOT DISTINCT FROM $7",
            )
            .bind(project_id)
            .bind(before.id)
            .bind(after.parent_id)
            .bind(&after.name)
            .bind(&after.path)
            .bind(&before.path)
            .bind(before.deletion_operation_id)
            .execute(conn)
            .await?;
            require_affected("update folder structure", result.rows_affected(), 1)
        }
        FileHistoryMutation::UpdateFileStructure { before, after } => {
            let result = sqlx::query(
                "UPDATE files SET folder_id = $3, name = $4, path = $5
                 WHERE project_id = $1 AND id = $2 AND path = $6
                   AND deletion_change_set_id IS NOT DISTINCT FROM $7",
            )
            .bind(project_id)
            .bind(before.id)
            .bind(after.folder_id)
            .bind(&after.name)
            .bind(&after.path)
            .bind(&before.path)
            .bind(before.deletion_operation_id)
            .execute(conn)
            .await?;
            require_affected("update file structure", result.rows_affected(), 1)
        }
        FileHistoryMutation::DeleteFolder {
            folder,
            operation_id,
        } => {
            if *operation_id != change_set_id {
                return Err(sqlx::Error::Protocol(
                    "delete folder operation does not own its change set".to_string(),
                ));
            }
            let result = sqlx::query(
                "UPDATE folders
                 SET deleted_at = $3, deleted_by = $4, purge_after = $5,
                     deletion_change_set_id = $6
                 WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
            )
            .bind(project_id)
            .bind(folder.id)
            .bind(effective_at)
            .bind(actor_id)
            .bind(purge_after)
            .bind(operation_id)
            .execute(conn)
            .await?;
            require_affected("delete folder", result.rows_affected(), 1)
        }
        FileHistoryMutation::DeleteFile { file, operation_id } => {
            if *operation_id != change_set_id {
                return Err(sqlx::Error::Protocol(
                    "delete file operation does not own its change set".to_string(),
                ));
            }
            let result = sqlx::query(
                "UPDATE files
                 SET deleted_at = $3, deleted_by = $4, purge_after = $5,
                     deletion_change_set_id = $6
                 WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
            )
            .bind(project_id)
            .bind(file.id)
            .bind(effective_at)
            .bind(actor_id)
            .bind(purge_after)
            .bind(operation_id)
            .execute(conn)
            .await?;
            require_affected("delete file", result.rows_affected(), 1)
        }
        FileHistoryMutation::RestoreFolder {
            folder,
            source_operation_id,
        } => {
            let result = sqlx::query(
                "UPDATE folders
                 SET deleted_at = NULL, deleted_by = NULL, purge_after = NULL,
                     deletion_change_set_id = NULL
                 WHERE project_id = $1 AND id = $2 AND deletion_change_set_id = $3",
            )
            .bind(project_id)
            .bind(folder.id)
            .bind(source_operation_id)
            .execute(conn)
            .await?;
            require_affected("restore folder", result.rows_affected(), 1)
        }
        FileHistoryMutation::RestoreFile {
            file,
            source_operation_id,
        } => {
            let result = sqlx::query(
                "UPDATE files
                 SET deleted_at = NULL, deleted_by = NULL, purge_after = NULL,
                     deletion_change_set_id = NULL
                 WHERE project_id = $1 AND id = $2 AND deletion_change_set_id = $3",
            )
            .bind(project_id)
            .bind(file.id)
            .bind(source_operation_id)
            .execute(conn)
            .await?;
            require_affected("restore file", result.rows_affected(), 1)
        }
        FileHistoryMutation::ReplaceEntry {
            entry_id,
            before,
            after,
        } => {
            let deleted_at = after.deleted.then_some(effective_at);
            let deletion_change_set_id = after.deleted.then_some(change_set_id);
            let result: Option<i64> = sqlx::query_scalar(
                "UPDATE entries
                 SET key = $4, original = $5, translation = $6, state = $7,
                     locked = $8, hidden = $9, deleted_at = $10,
                     deleted_by = CASE WHEN $10::TIMESTAMPTZ IS NULL THEN NULL ELSE $11 END,
                     deletion_change_set_id = $12, version = version + 1,
                     updated_by = $11
                 WHERE project_id = $1 AND id = $2 AND key = $3
                 RETURNING version",
            )
            .bind(project_id)
            .bind(entry_id)
            .bind(&before.key)
            .bind(&after.key)
            .bind(json!(after.original))
            .bind(&after.translation)
            .bind(after.state.as_str())
            .bind(after.locked)
            .bind(after.hidden)
            .bind(deleted_at)
            .bind(actor_id)
            .bind(deletion_change_set_id)
            .fetch_optional(&mut *conn)
            .await?;
            let version = result.ok_or_else(|| {
                sqlx::Error::Protocol("rollback entry precondition changed".to_string())
            })?;
            sqlx::query(
                "INSERT INTO entry_versions (
                     entry_id, version, kind, translation, state, original, editor_id,
                     editor_name, editor_avatar_url
                 )
                 SELECT $1, $2, 'rollback', $3, $4, $5, actor.id, actor.username,
                        actor.avatar_url
                 FROM (SELECT 1) AS seed LEFT JOIN users AS actor ON actor.id = $6",
            )
            .bind(entry_id)
            .bind(version)
            .bind(&after.translation)
            .bind(after.state.as_str())
            .bind(json!(after.original))
            .bind(actor_id)
            .execute(conn)
            .await
            .map(|_| ())
        }
    }
}

async fn apply_stats_delta_tx(
    conn: &mut PgConnection,
    project_id: i64,
    project_delta: MaterializedFileStats,
    file_delta: Option<(i64, MaterializedFileStats)>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE project_stats SET
             visible_total = visible_total + $2,
             untranslated_count = untranslated_count + $3,
             translated_count = translated_count + $4,
             questioned_count = questioned_count + $5,
             checked_count = checked_count + $6,
             reviewed_count = reviewed_count + $7,
             hidden_total = hidden_total + $8,
             hidden_untranslated_count = hidden_untranslated_count + $9,
             hidden_translated_count = hidden_translated_count + $10,
             hidden_questioned_count = hidden_questioned_count + $11,
             hidden_checked_count = hidden_checked_count + $12,
             hidden_reviewed_count = hidden_reviewed_count + $13,
             updated_at = now()
         WHERE project_id = $1",
    )
    .bind(project_id)
    .bind(project_delta.visible_total)
    .bind(project_delta.untranslated)
    .bind(project_delta.translated)
    .bind(project_delta.questioned)
    .bind(project_delta.checked)
    .bind(project_delta.reviewed)
    .bind(project_delta.hidden_total)
    .bind(project_delta.hidden_untranslated)
    .bind(project_delta.hidden_translated)
    .bind(project_delta.hidden_questioned)
    .bind(project_delta.hidden_checked)
    .bind(project_delta.hidden_reviewed)
    .execute(&mut *conn)
    .await?;
    if let Some((file_id, delta)) = file_delta {
        sqlx::query(
            "UPDATE file_stats SET
                 visible_total = visible_total + $2,
                 untranslated_count = untranslated_count + $3,
                 translated_count = translated_count + $4,
                 questioned_count = questioned_count + $5,
                 checked_count = checked_count + $6,
                 reviewed_count = reviewed_count + $7,
                 hidden_total = hidden_total + $8,
                 hidden_untranslated_count = hidden_untranslated_count + $9,
                 hidden_translated_count = hidden_translated_count + $10,
                 hidden_questioned_count = hidden_questioned_count + $11,
                 hidden_checked_count = hidden_checked_count + $12,
                 hidden_reviewed_count = hidden_reviewed_count + $13,
                 updated_at = now()
             WHERE file_id = $1",
        )
        .bind(file_id)
        .bind(delta.visible_total)
        .bind(delta.untranslated)
        .bind(delta.translated)
        .bind(delta.questioned)
        .bind(delta.checked)
        .bind(delta.reviewed)
        .bind(delta.hidden_total)
        .bind(delta.hidden_untranslated)
        .bind(delta.hidden_translated)
        .bind(delta.hidden_questioned)
        .bind(delta.hidden_checked)
        .bind(delta.hidden_reviewed)
        .execute(conn)
        .await?;
    }
    Ok(())
}

async fn verify_stats_nonnegative_tx(
    conn: &mut PgConnection,
    project_id: i64,
    file_id: Option<i64>,
) -> Result<(), sqlx::Error> {
    let project_valid: bool = sqlx::query_scalar(
        "SELECT visible_total >= 0
            AND visible_total = untranslated_count + translated_count
                + questioned_count + checked_count + reviewed_count
            AND hidden_total >= 0
            AND hidden_total = hidden_untranslated_count + hidden_translated_count
                + hidden_questioned_count + hidden_checked_count + hidden_reviewed_count
         FROM project_stats WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_one(&mut *conn)
    .await?;
    if !project_valid {
        return Err(sqlx::Error::Protocol(
            "file history project stats postcondition failed".to_string(),
        ));
    }
    if let Some(file_id) = file_id {
        let file_valid: bool = sqlx::query_scalar(
            "SELECT visible_total >= 0
                AND visible_total = untranslated_count + translated_count
                    + questioned_count + checked_count + reviewed_count
                AND hidden_total >= 0
                AND hidden_total = hidden_untranslated_count + hidden_translated_count
                    + hidden_questioned_count + hidden_checked_count + hidden_reviewed_count
             FROM file_stats WHERE file_id = $1",
        )
        .bind(file_id)
        .fetch_one(conn)
        .await?;
        if !file_valid {
            return Err(sqlx::Error::Protocol(
                "file history file stats postcondition failed".to_string(),
            ));
        }
    }
    Ok(())
}

#[derive(Debug)]
struct PreparedHistoryRow {
    entity_type: &'static str,
    entity_id: i64,
    operation: &'static str,
    before_value: Option<Value>,
    after_value: Option<Value>,
    ordinal: i32,
}

async fn prepare_history_rows(
    conn: &mut PgConnection,
    plan: &FileHistoryPlan,
    effective_at: DateTime<Utc>,
) -> Result<Vec<PreparedHistoryRow>, sqlx::Error> {
    let mut folder_ids = Vec::new();
    let mut file_ids = Vec::new();
    let mut entry_ids = Vec::new();
    for delta in &plan.history {
        match delta.entity {
            FileHistoryEntity::Folder => folder_ids.push(delta.entity_id),
            FileHistoryEntity::File => file_ids.push(delta.entity_id),
            FileHistoryEntity::Entry => entry_ids.push(delta.entity_id),
        }
    }
    let mut deleted_at = HashMap::<(FileHistoryEntity, i64), Option<DateTime<Utc>>>::new();
    load_deleted_at_tx(
        &mut *conn,
        "folders",
        FileHistoryEntity::Folder,
        &folder_ids,
        &mut deleted_at,
    )
    .await?;
    load_deleted_at_tx(
        &mut *conn,
        "files",
        FileHistoryEntity::File,
        &file_ids,
        &mut deleted_at,
    )
    .await?;
    load_deleted_at_tx(
        conn,
        "entries",
        FileHistoryEntity::Entry,
        &entry_ids,
        &mut deleted_at,
    )
    .await?;

    plan.history
        .iter()
        .map(|delta| {
            let current_deleted_at = deleted_at
                .get(&(delta.entity, delta.entity_id))
                .copied()
                .flatten();
            let before_deleted = delta.before.as_ref().is_some_and(snapshot_deleted);
            Ok(PreparedHistoryRow {
                entity_type: entity_name(delta.entity),
                entity_id: delta.entity_id,
                operation: item_operation_name(delta.operation),
                before_value: delta.before.as_ref().map(|snapshot| {
                    snapshot_json(
                        snapshot,
                        current_deleted_at,
                        effective_at,
                        false,
                        before_deleted,
                    )
                }),
                after_value: delta.after.as_ref().map(|snapshot| {
                    snapshot_json(
                        snapshot,
                        current_deleted_at,
                        effective_at,
                        true,
                        before_deleted,
                    )
                }),
                ordinal: delta.ordinal,
            })
        })
        .collect()
}

async fn load_deleted_at_tx(
    conn: &mut PgConnection,
    table: &'static str,
    entity: FileHistoryEntity,
    ids: &[i64],
    output: &mut HashMap<(FileHistoryEntity, i64), Option<DateTime<Utc>>>,
) -> Result<(), sqlx::Error> {
    if ids.is_empty() {
        return Ok(());
    }
    let statement = match table {
        "folders" => "SELECT id, deleted_at FROM folders WHERE id = ANY($1::BIGINT[])",
        "files" => "SELECT id, deleted_at FROM files WHERE id = ANY($1::BIGINT[])",
        "entries" => "SELECT id, deleted_at FROM entries WHERE id = ANY($1::BIGINT[])",
        _ => unreachable!("allowlisted history table"),
    };
    for (id, value) in sqlx::query_as::<_, (i64, Option<DateTime<Utc>>)>(statement)
        .bind(ids)
        .fetch_all(conn)
        .await?
    {
        output.insert((entity, id), value);
    }
    Ok(())
}

fn snapshot_json(
    snapshot: &FileHistorySnapshot,
    current_deleted_at: Option<DateTime<Utc>>,
    effective_at: DateTime<Utc>,
    after: bool,
    before_deleted: bool,
) -> Value {
    let deleted_at = if snapshot_deleted(snapshot) {
        if after && !before_deleted {
            Some(effective_at)
        } else {
            current_deleted_at.or(Some(effective_at))
        }
    } else {
        None
    };
    match snapshot {
        FileHistorySnapshot::Folder(value) => json!({
            "parent_id": value.parent_id,
            "name": value.name,
            "path": value.path,
            "deleted_at": deleted_at,
        }),
        FileHistorySnapshot::File(value) => json!({
            "folder_id": value.folder_id,
            "name": value.name,
            "path": value.path,
            "deleted_at": deleted_at,
        }),
        FileHistorySnapshot::Entry(value) => json!({
            "key": value.key,
            "original": value.original,
            "translation": value.translation,
            "state": value.state.as_str(),
            "locked": value.locked,
            "hidden": value.hidden,
            "deleted_at": deleted_at,
        }),
    }
}

fn snapshot_deleted(snapshot: &FileHistorySnapshot) -> bool {
    match snapshot {
        FileHistorySnapshot::Folder(value) => value.deleted,
        FileHistorySnapshot::File(value) => value.deleted,
        FileHistorySnapshot::Entry(value) => value.deleted,
    }
}

fn operation_name(operation: FileHistoryOperation) -> &'static str {
    match operation {
        FileHistoryOperation::Move => "move",
        FileHistoryOperation::Rename => "rename",
        FileHistoryOperation::Delete => "delete",
        FileHistoryOperation::Restore => "restore",
        FileHistoryOperation::Rollback => "rollback",
    }
}

fn entity_name(entity: FileHistoryEntity) -> &'static str {
    match entity {
        FileHistoryEntity::Folder => "folder",
        FileHistoryEntity::File => "file",
        FileHistoryEntity::Entry => "entry",
    }
}

fn item_operation_name(operation: FileHistoryItemOperation) -> &'static str {
    match operation {
        FileHistoryItemOperation::Update => "update",
        FileHistoryItemOperation::Move => "move",
        FileHistoryItemOperation::Delete => "delete",
        FileHistoryItemOperation::Restore => "restore",
        FileHistoryItemOperation::Tombstone => "tombstone",
    }
}

fn stats_json(stats: MaterializedFileStats) -> Value {
    json!({
        "visible_total": stats.visible_total,
        "untranslated": stats.untranslated,
        "translated": stats.translated,
        "questioned": stats.questioned,
        "checked": stats.checked,
        "reviewed": stats.reviewed,
        "hidden_total": stats.hidden_total,
        "hidden_untranslated": stats.hidden_untranslated,
        "hidden_translated": stats.hidden_translated,
        "hidden_questioned": stats.hidden_questioned,
        "hidden_checked": stats.hidden_checked,
        "hidden_reviewed": stats.hidden_reviewed,
    })
}

fn require_affected(operation: &str, actual: u64, expected: u64) -> Result<(), sqlx::Error> {
    if actual == expected {
        Ok(())
    } else {
        Err(sqlx::Error::Protocol(format!(
            "file history {operation} affected {actual} rows, expected {expected}"
        )))
    }
}
