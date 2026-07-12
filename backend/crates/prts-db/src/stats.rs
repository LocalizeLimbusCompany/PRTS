//! Effective-visible 项目与文件物化统计。

use sqlx::{PgConnection, PgPool};

use crate::models::{FileStats, ProjectStats};

/// 读取项目统计；迁移 trigger 保证每个项目都有一行。
pub async fn project(pool: &PgPool, project_id: i64) -> Result<ProjectStats, sqlx::Error> {
    sqlx::query_as("SELECT * FROM project_stats WHERE project_id = $1")
        .bind(project_id)
        .fetch_one(pool)
        .await
}

/// 读取项目全部 active 文件统计。
pub async fn files(pool: &PgPool, project_id: i64) -> Result<Vec<FileStats>, sqlx::Error> {
    sqlx::query_as(
        "SELECT stats.* FROM file_stats AS stats
         JOIN files AS file ON file.id = stats.file_id
         WHERE stats.project_id = $1 AND file.deleted_at IS NULL
         ORDER BY stats.file_id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}

/// 低频修复入口：按规范谓词重建单个项目统计，不用于正常读取。
pub async fn rebuild_project_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<ProjectStats, sqlx::Error> {
    sqlx::query(
        "INSERT INTO file_stats (
             file_id, project_id, visible_total, untranslated_count, translated_count,
             questioned_count, checked_count, reviewed_count, updated_at
         )
         SELECT file.id, file.project_id,
                count(entry.id),
                count(entry.id) FILTER (WHERE entry.state = 'untranslated'),
                count(entry.id) FILTER (WHERE entry.state = 'translated'),
                count(entry.id) FILTER (WHERE entry.state = 'questioned'),
                count(entry.id) FILTER (WHERE entry.state = 'checked'),
                count(entry.id) FILTER (WHERE entry.state = 'reviewed'), now()
         FROM files AS file
         LEFT JOIN entries AS entry
           ON entry.file_id = file.id AND prts_entry_is_effectively_visible(entry)
         WHERE file.project_id = $1
         GROUP BY file.id, file.project_id
         ON CONFLICT (file_id) DO UPDATE SET
             project_id = EXCLUDED.project_id,
             visible_total = EXCLUDED.visible_total,
             untranslated_count = EXCLUDED.untranslated_count,
             translated_count = EXCLUDED.translated_count,
             questioned_count = EXCLUDED.questioned_count,
             checked_count = EXCLUDED.checked_count,
             reviewed_count = EXCLUDED.reviewed_count,
             updated_at = now()",
    )
    .bind(project_id)
    .execute(&mut *conn)
    .await?;

    sqlx::query_as(
        "INSERT INTO project_stats (
             project_id, visible_total, untranslated_count, translated_count,
             questioned_count, checked_count, reviewed_count, updated_at
         )
         SELECT $1,
                COALESCE(sum(visible_total), 0),
                COALESCE(sum(untranslated_count), 0),
                COALESCE(sum(translated_count), 0),
                COALESCE(sum(questioned_count), 0),
                COALESCE(sum(checked_count), 0),
                COALESCE(sum(reviewed_count), 0), now()
         FROM file_stats WHERE project_id = $1
         ON CONFLICT (project_id) DO UPDATE SET
             visible_total = EXCLUDED.visible_total,
             untranslated_count = EXCLUDED.untranslated_count,
             translated_count = EXCLUDED.translated_count,
             questioned_count = EXCLUDED.questioned_count,
             checked_count = EXCLUDED.checked_count,
             reviewed_count = EXCLUDED.reviewed_count,
             updated_at = now()
         RETURNING *",
    )
    .bind(project_id)
    .fetch_one(conn)
    .await
}
