//! 平台双语 POS 预设仓储。

use sqlx::{PgConnection, PgPool};

use crate::models::PosPreset;

/// 按 sort_order/id 稳定列出全部 POS；POS 数量是平台级小集合，不使用 OFFSET。
pub async fn list(pool: &PgPool) -> Result<Vec<PosPreset>, sqlx::Error> {
    sqlx::query_as::<_, PosPreset>("SELECT * FROM pos_presets ORDER BY sort_order ASC, id ASC")
        .fetch_all(pool)
        .await
}

/// Confirm 时先取得平台级 advisory lock，再锁定全部 POS，串行化批量 identity 计划。
pub async fn list_for_import_tx(conn: &mut PgConnection) -> Result<Vec<PosPreset>, sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(578_084_595_650_329_i64)
        .execute(&mut *conn)
        .await?;
    sqlx::query_as::<_, PosPreset>(
        "SELECT * FROM pos_presets ORDER BY sort_order ASC, id ASC FOR UPDATE",
    )
    .fetch_all(conn)
    .await
}

/// Term confirm 锁住当前 POS 小集合，避免解析后被并发删除或改名。
pub async fn list_for_term_import_tx(
    conn: &mut PgConnection,
) -> Result<Vec<PosPreset>, sqlx::Error> {
    sqlx::query_as::<_, PosPreset>(
        "SELECT * FROM pos_presets ORDER BY sort_order ASC, id ASC FOR UPDATE",
    )
    .fetch_all(conn)
    .await
}

/// 在 mutation 事务中锁定一个 POS。
pub async fn find_for_update_tx(
    conn: &mut PgConnection,
    id: i64,
) -> Result<Option<PosPreset>, sqlx::Error> {
    sqlx::query_as::<_, PosPreset>("SELECT * FROM pos_presets WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// 验证可选 POS 引用存在；NULL 始终合法。
pub async fn exists_tx(conn: &mut PgConnection, id: Option<i64>) -> Result<bool, sqlx::Error> {
    match id {
        Some(id) => {
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pos_presets WHERE id = $1)")
                .bind(id)
                .fetch_one(conn)
                .await
        }
        None => Ok(true),
    }
}

/// 统计删除 POS 时将通过 SET NULL 脱离的术语数量。
pub async fn count_references_tx(conn: &mut PgConnection, id: i64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM terms WHERE pos_id = $1")
        .bind(id)
        .fetch_one(conn)
        .await
}

/// 在调用方事务内创建 POS。
pub async fn create_tx(
    conn: &mut PgConnection,
    name_zh_cn: Option<&str>,
    name_en: Option<&str>,
    sort_order: i32,
) -> Result<PosPreset, sqlx::Error> {
    sqlx::query_as::<_, PosPreset>(
        "INSERT INTO pos_presets (name_zh_cn, name_en, sort_order)
         VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(name_zh_cn)
    .bind(name_en)
    .bind(sort_order)
    .fetch_one(conn)
    .await
}

/// 在调用方事务内更新 POS。
pub async fn update_tx(
    conn: &mut PgConnection,
    id: i64,
    name_zh_cn: Option<&str>,
    name_en: Option<&str>,
    sort_order: i32,
) -> Result<Option<PosPreset>, sqlx::Error> {
    sqlx::query_as::<_, PosPreset>(
        "UPDATE pos_presets
         SET name_zh_cn = $2, name_en = $3, sort_order = $4
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(name_zh_cn)
    .bind(name_en)
    .bind(sort_order)
    .fetch_optional(conn)
    .await
}

/// 删除 POS；terms.pos_id 的显式 SET NULL 保留术语。
pub async fn delete_tx(conn: &mut PgConnection, id: i64) -> Result<bool, sqlx::Error> {
    sqlx::query("DELETE FROM pos_presets WHERE id = $1")
        .bind(id)
        .execute(conn)
        .await
        .map(|result| result.rows_affected() == 1)
}
