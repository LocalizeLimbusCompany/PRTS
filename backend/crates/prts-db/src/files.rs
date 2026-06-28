//! 文件夹与文件数据访问。

use sqlx::PgPool;

use crate::models::{File, Folder};

/// 取得或创建某路径的文件夹，返回其 id。
async fn get_or_create_folder(
    pool: &PgPool,
    project_id: i64,
    parent_id: Option<i64>,
    name: &str,
    path: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "INSERT INTO folders (project_id, parent_id, name, path) VALUES ($1, $2, $3, $4)
         ON CONFLICT (project_id, path) DO UPDATE SET name = EXCLUDED.name
         RETURNING id",
    )
    .bind(project_id)
    .bind(parent_id)
    .bind(name)
    .bind(path)
    .fetch_one(pool)
    .await
}

/// 按全路径（如 `a/b/c.json`）确保文件存在：沿途创建文件夹与文件，返回文件。
/// 调用方需保证 `full_path` 至少含一个文件名段。
pub async fn ensure_file_at_path(
    pool: &PgPool,
    project_id: i64,
    full_path: &str,
) -> Result<File, sqlx::Error> {
    let parts: Vec<&str> = full_path.split('/').filter(|s| !s.is_empty()).collect();
    let (file_name, folder_parts) = parts
        .split_last()
        .expect("路径需含文件名段（调用方已校验）");

    let mut parent_id: Option<i64> = None;
    let mut cur_path = String::new();
    for seg in folder_parts {
        cur_path = if cur_path.is_empty() {
            (*seg).to_string()
        } else {
            format!("{cur_path}/{seg}")
        };
        let id = get_or_create_folder(pool, project_id, parent_id, seg, &cur_path).await?;
        parent_id = Some(id);
    }

    sqlx::query_as::<_, File>(
        "INSERT INTO files (project_id, folder_id, name, path) VALUES ($1, $2, $3, $4)
         ON CONFLICT (project_id, path) DO UPDATE SET name = EXCLUDED.name
         RETURNING *",
    )
    .bind(project_id)
    .bind(parent_id)
    .bind(file_name)
    .bind(full_path)
    .fetch_one(pool)
    .await
}

/// 按 id 查文件（限定项目）。
pub async fn find_file(
    pool: &PgPool,
    project_id: i64,
    file_id: i64,
) -> Result<Option<File>, sqlx::Error> {
    sqlx::query_as::<_, File>("SELECT * FROM files WHERE id = $1 AND project_id = $2")
        .bind(file_id)
        .bind(project_id)
        .fetch_optional(pool)
        .await
}

/// 列出项目的全部文件夹。
pub async fn list_folders(pool: &PgPool, project_id: i64) -> Result<Vec<Folder>, sqlx::Error> {
    sqlx::query_as::<_, Folder>("SELECT * FROM folders WHERE project_id = $1 ORDER BY path")
        .bind(project_id)
        .fetch_all(pool)
        .await
}

/// 列出项目的全部文件。
pub async fn list_files(pool: &PgPool, project_id: i64) -> Result<Vec<File>, sqlx::Error> {
    sqlx::query_as::<_, File>("SELECT * FROM files WHERE project_id = $1 ORDER BY path")
        .bind(project_id)
        .fetch_all(pool)
        .await
}

/// 删除文件（级联词条）。
pub async fn delete_file(
    pool: &PgPool,
    project_id: i64,
    file_id: i64,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM files WHERE id = $1 AND project_id = $2")
        .bind(file_id)
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// 删除文件夹（级联子文件夹/文件/词条）。
pub async fn delete_folder(
    pool: &PgPool,
    project_id: i64,
    folder_id: i64,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM folders WHERE id = $1 AND project_id = $2")
        .bind(folder_id)
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// 重算并更新文件的词条计数。
pub async fn refresh_entry_count(pool: &PgPool, file_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE files SET entry_count = (SELECT COUNT(*) FROM entries WHERE file_id = $1) WHERE id = $1",
    )
    .bind(file_id)
    .execute(pool)
    .await
    .map(|_| ())
}
