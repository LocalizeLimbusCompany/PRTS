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

/// 从物化统计读取编辑器浏览范围的精确可见词条总数。
///
/// 工作流状态或 questioned overlay 只读取固定物化列；文件/任务范围均不扫描 entries 热表。
pub async fn editor_entry_total(
    pool: &PgPool,
    project_id: i64,
    file_id: Option<i64>,
    task_id: Option<i64>,
    states: &[String],
    questioned: Option<bool>,
    include_hidden: bool,
) -> Result<i64, sqlx::Error> {
    let expression = editor_stats_expression(states, questioned, include_hidden);
    if let Some(file_id) = file_id {
        return sqlx::query_scalar(&format!(
            "SELECT COALESCE({expression}, 0) FROM file_stats AS stats
             JOIN files AS file ON file.id = stats.file_id
             WHERE stats.project_id = $1 AND stats.file_id = $2
               AND file.deleted_at IS NULL"
        ))
        .bind(project_id)
        .bind(file_id)
        .fetch_optional(pool)
        .await
        .map(|value| value.unwrap_or(0));
    }
    if let Some(task_id) = task_id {
        return sqlx::query_scalar(&format!(
            "SELECT COALESCE(sum({expression}), 0)::BIGINT
             FROM task_files AS task_file
             JOIN file_stats AS stats ON stats.file_id = task_file.live_file_id
             JOIN files AS file ON file.id = stats.file_id
             WHERE task_file.task_id = $2 AND stats.project_id = $1
               AND file.deleted_at IS NULL
               AND NOT EXISTS (
                   SELECT 1 FROM folders AS ancestor
                   WHERE ancestor.project_id = file.project_id
                     AND ancestor.deleted_at IS NOT NULL
                     AND (file.path = ancestor.path
                          OR file.path LIKE prts_escape_like_pattern(ancestor.path) || '/%' ESCAPE '\\')
               )"
        ))
        .bind(project_id)
        .bind(task_id)
        .fetch_one(pool)
        .await;
    }
    sqlx::query_scalar(&format!(
        "SELECT COALESCE({expression}, 0) FROM project_stats AS stats WHERE project_id = $1"
    ))
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map(|value| value.unwrap_or(0))
}

/// 仅从固定状态白名单组合物化列表达式，不接受任意 SQL 片段。
fn editor_stats_expression(
    states: &[String],
    questioned: Option<bool>,
    include_hidden: bool,
) -> String {
    let columns = if states.is_empty() {
        match (questioned, include_hidden) {
            (None, false) => "stats.visible_total".to_string(),
            (None, true) => "stats.visible_total + stats.hidden_total".to_string(),
            (Some(true), false) => "stats.questioned_count".to_string(),
            (Some(true), true) => {
                "stats.questioned_count + stats.hidden_questioned_count".to_string()
            }
            (Some(false), false) => "stats.visible_total - stats.questioned_count".to_string(),
            (Some(false), true) => "(stats.visible_total - stats.questioned_count) + \
                (stats.hidden_total - stats.hidden_questioned_count)"
                .to_string(),
        }
    } else {
        states
            .iter()
            .filter_map(|state| match state.as_str() {
                "untranslated" => Some((
                    "stats.untranslated_count",
                    "stats.hidden_untranslated_count",
                )),
                "translated" => Some(("stats.translated_count", "stats.hidden_translated_count")),
                "checked" => Some(("stats.checked_count", "stats.hidden_checked_count")),
                "reviewed" => Some(("stats.reviewed_count", "stats.hidden_reviewed_count")),
                _ => None,
            })
            .map(|(visible, hidden)| {
                if include_hidden {
                    format!("({visible} + {hidden})")
                } else {
                    visible.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" + ")
    };
    if columns.is_empty() {
        "0".to_string()
    } else {
        columns
    }
}

/// 低频修复入口：重建文件内在词条计数，再按 active tree exposure 聚合项目统计。
///
/// 删除文件的 `file_stats` 保留以便恢复，`project_stats` 则始终只包含有效可见文件。
pub async fn rebuild_project_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<ProjectStats, sqlx::Error> {
    sqlx::query(
        "INSERT INTO file_stats (
             file_id, project_id, visible_total, untranslated_count, translated_count,
             questioned_count, checked_count, reviewed_count, hidden_total,
             hidden_untranslated_count, hidden_translated_count,
             hidden_questioned_count, hidden_checked_count, hidden_reviewed_count, updated_at
         )
         SELECT file.id, file.project_id,
                count(entry.id) FILTER (
                    WHERE entry.deleted_at IS NULL AND NOT entry.hidden
                ),
                count(entry.id) FILTER (
                    WHERE entry.deleted_at IS NULL AND NOT entry.hidden
                      AND entry.state = 'untranslated'
                ),
                count(entry.id) FILTER (
                    WHERE entry.deleted_at IS NULL AND NOT entry.hidden
                      AND entry.state = 'translated'
                ),
                count(entry.id) FILTER (
                    WHERE entry.deleted_at IS NULL AND NOT entry.hidden
                      AND entry.questioned
                ),
                count(entry.id) FILTER (
                    WHERE entry.deleted_at IS NULL AND NOT entry.hidden
                      AND entry.state = 'checked'
                ),
                count(entry.id) FILTER (
                    WHERE entry.deleted_at IS NULL AND NOT entry.hidden
                      AND entry.state = 'reviewed'
                ),
                count(entry.id) FILTER (
                    WHERE entry.deleted_at IS NULL AND entry.hidden
                ),
                count(entry.id) FILTER (
                    WHERE entry.deleted_at IS NULL AND entry.hidden
                      AND entry.state = 'untranslated'
                ),
                count(entry.id) FILTER (
                    WHERE entry.deleted_at IS NULL AND entry.hidden
                      AND entry.state = 'translated'
                ),
                count(entry.id) FILTER (
                    WHERE entry.deleted_at IS NULL AND entry.hidden
                      AND entry.questioned
                ),
                count(entry.id) FILTER (
                    WHERE entry.deleted_at IS NULL AND entry.hidden
                      AND entry.state = 'checked'
                ),
                count(entry.id) FILTER (
                    WHERE entry.deleted_at IS NULL AND entry.hidden
                      AND entry.state = 'reviewed'
                ), now()
         FROM files AS file
         LEFT JOIN entries AS entry ON entry.file_id = file.id
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
             hidden_total = EXCLUDED.hidden_total,
             hidden_untranslated_count = EXCLUDED.hidden_untranslated_count,
             hidden_translated_count = EXCLUDED.hidden_translated_count,
             hidden_questioned_count = EXCLUDED.hidden_questioned_count,
             hidden_checked_count = EXCLUDED.hidden_checked_count,
             hidden_reviewed_count = EXCLUDED.hidden_reviewed_count,
             updated_at = now()",
    )
    .bind(project_id)
    .execute(&mut *conn)
    .await?;

    sqlx::query_as(
        "INSERT INTO project_stats (
             project_id, visible_total, untranslated_count, translated_count,
             questioned_count, checked_count, reviewed_count, hidden_total,
             hidden_untranslated_count, hidden_translated_count,
             hidden_questioned_count, hidden_checked_count, hidden_reviewed_count, updated_at
         )
         SELECT $1,
                COALESCE(sum(stats.visible_total), 0),
                COALESCE(sum(stats.untranslated_count), 0),
                COALESCE(sum(stats.translated_count), 0),
                COALESCE(sum(stats.questioned_count), 0),
                COALESCE(sum(stats.checked_count), 0),
                COALESCE(sum(stats.reviewed_count), 0),
                COALESCE(sum(stats.hidden_total), 0),
                COALESCE(sum(stats.hidden_untranslated_count), 0),
                COALESCE(sum(stats.hidden_translated_count), 0),
                COALESCE(sum(stats.hidden_questioned_count), 0),
                COALESCE(sum(stats.hidden_checked_count), 0),
                COALESCE(sum(stats.hidden_reviewed_count), 0), now()
         FROM file_stats AS stats
         JOIN files AS file ON file.id = stats.file_id
         WHERE stats.project_id = $1 AND file.deleted_at IS NULL
           AND NOT EXISTS (
               SELECT 1 FROM folders AS ancestor
               WHERE ancestor.project_id = file.project_id
                 AND ancestor.deleted_at IS NOT NULL
                 AND (file.path = ancestor.path
                      OR file.path LIKE prts_escape_like_pattern(ancestor.path) || '/%' ESCAPE '\\')
           )
         ON CONFLICT (project_id) DO UPDATE SET
             visible_total = EXCLUDED.visible_total,
             untranslated_count = EXCLUDED.untranslated_count,
             translated_count = EXCLUDED.translated_count,
             questioned_count = EXCLUDED.questioned_count,
             checked_count = EXCLUDED.checked_count,
             reviewed_count = EXCLUDED.reviewed_count,
             hidden_total = EXCLUDED.hidden_total,
             hidden_untranslated_count = EXCLUDED.hidden_untranslated_count,
             hidden_translated_count = EXCLUDED.hidden_translated_count,
             hidden_questioned_count = EXCLUDED.hidden_questioned_count,
             hidden_checked_count = EXCLUDED.hidden_checked_count,
             hidden_reviewed_count = EXCLUDED.hidden_reviewed_count,
             updated_at = now()
         RETURNING *",
    )
    .bind(project_id)
    .fetch_one(conn)
    .await
}

#[cfg(test)]
mod tests {
    use super::editor_stats_expression;

    #[test]
    fn editor_total_expression_uses_only_materialized_visible_and_hidden_columns() {
        assert_eq!(
            editor_stats_expression(&[], None, false),
            "stats.visible_total"
        );
        assert_eq!(
            editor_stats_expression(&[], None, true),
            "stats.visible_total + stats.hidden_total"
        );
        assert_eq!(
            editor_stats_expression(&[], Some(true), true),
            "stats.questioned_count + stats.hidden_questioned_count"
        );
        assert_eq!(
            editor_stats_expression(
                &["translated".to_string(), "checked".to_string()],
                None,
                true
            ),
            "(stats.translated_count + stats.hidden_translated_count) + \
(stats.checked_count + stats.hidden_checked_count)"
        );
        assert_eq!(
            editor_stats_expression(&["invalid".to_string()], None, true),
            "0"
        );
    }

    #[test]
    fn editor_collaboration_migration_declares_hidden_stats_and_trigger_maintenance() {
        let migration = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../migrations/0016_editor_collaboration.sql"),
        )
        .expect("0016 editor collaboration migration must exist");
        for required in [
            "ADD COLUMN hidden_total",
            "project_stats_hidden_sum_chk",
            "file_stats_hidden_sum_chk",
            "prts_apply_entry_stats_delta_v2",
            "prts_entry_structurally_visible",
            "CREATE OR REPLACE FUNCTION maintain_entry_stats",
            "CREATE OR REPLACE FUNCTION subtract_file_stats_before_delete",
        ] {
            assert!(migration.contains(required), "0016 missing {required}");
        }
    }
}
