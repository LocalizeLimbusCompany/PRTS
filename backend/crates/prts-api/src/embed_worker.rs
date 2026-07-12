//! 后台嵌入 sweep：把 embedding IS NULL 的词条分批向量化。开关/配置每轮重读，
//! 故管理后台开关向量化后无需重启即可生效。
use std::{sync::Arc, time::Duration};

use prts_db::search_settings::SearchConfig;
use prts_search::qwen::QwenProvider;
use sqlx::PgPool;
use tokio::sync::RwLock;

const IDLE: Duration = Duration::from_secs(30);
const ACTIVE: Duration = Duration::from_secs(1);
const MAX_ATTEMPTS: i16 = 5;
const SELECT_LIMIT: i64 = 50;

/// 启动 sweep 后台任务。仅当向量化启用且 env 配了 key 时才实际嵌入。
pub fn spawn(db: PgPool, embedder: Arc<Option<QwenProvider>>, rt: Arc<RwLock<SearchConfig>>) {
    tokio::spawn(async move {
        loop {
            let cfg = rt.read().await.clone();
            let provider = match (cfg.embedding_enabled, embedder.as_ref()) {
                (true, Some(p)) => p,
                _ => {
                    tokio::time::sleep(IDLE).await;
                    continue;
                }
            };
            let rows: Vec<(i64, String)> = match sqlx::query_as(
                "SELECT entry.id, entry.source_text FROM entries AS entry
                 JOIN projects AS project ON project.id = entry.project_id
                 WHERE entry.embedding IS NULL AND entry.source_text <> ''
                   AND entry.embed_attempts < $1
                   AND project.language_repair_state = 'ready'
                   AND project.lexical_state = 'ready'
                   AND project.embedding_job_id IS NULL
                 ORDER BY entry.id LIMIT $2",
            )
            .bind(MAX_ATTEMPTS)
            .bind(SELECT_LIMIT)
            .fetch_all(&db)
            .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("sweep select failed: {e}");
                    tokio::time::sleep(IDLE).await;
                    continue;
                }
            };
            if rows.is_empty() {
                tokio::time::sleep(IDLE).await;
                continue;
            }

            let batch = (cfg.embedding_batch.clamp(1, 10)) as usize;
            for chunk in rows.chunks(batch) {
                let texts: Vec<String> = chunk.iter().map(|(_, t)| t.clone()).collect();
                match provider
                    .embed_batch(&cfg.embedding_base_url, &cfg.embedding_model, &texts)
                    .await
                {
                    Ok(vecs) => {
                        for ((id, captured), vec) in chunk.iter().zip(vecs) {
                            let v = pgvector::Vector::from(vec);
                            // 乐观：仅当 source_text 仍是抓取时的值才写，避免覆盖并发改源。
                            let _ = sqlx::query(
                                "UPDATE entries SET embedding = $1, embed_attempts = 0
                                 WHERE id = $2 AND source_text = $3",
                            )
                            .bind(v)
                            .bind(id)
                            .bind(captured)
                            .execute(&db)
                            .await;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("embed batch failed: {e}");
                        let ids: Vec<i64> = chunk.iter().map(|(id, _)| *id).collect();
                        let _ = sqlx::query(
                            "UPDATE entries SET embed_attempts = embed_attempts + 1 WHERE id = ANY($1)",
                        )
                        .bind(&ids)
                        .execute(&db)
                        .await;
                    }
                }
            }
            tokio::time::sleep(ACTIVE).await; // Qwen QPS 节流
        }
    });
}
