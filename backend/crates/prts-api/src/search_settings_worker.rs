//! 搜索运行时设置的单写者队列。
//!
//! HTTP 请求只负责入队并等待结果。请求 future 即使被取消，worker 仍会完成已经接收的
//! 数据库事务、审计与 runtime 发布，避免 commit 边界取消造成持久化与内存分叉。

use std::sync::Arc;

use prts_common::Error;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};
use prts_db::search_settings::SearchConfig;

#[derive(Clone)]
pub struct SearchSettingsUpdater {
    sender: tokio::sync::mpsc::Sender<UpdateRequest>,
    shutdown: Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

struct UpdateRequest {
    actor_id: i64,
    config: SearchConfig,
    reply: tokio::sync::oneshot::Sender<Result<SearchConfig, Error>>,
}

impl SearchSettingsUpdater {
    /// 将更新交给受监督的单写者 worker，并返回仅承载响应的接收端。
    ///
    /// `send` 成功即表示 worker 拥有该更新；之后丢弃接收端不会取消更新。
    pub(crate) async fn enqueue(
        &self,
        actor_id: i64,
        config: SearchConfig,
    ) -> Result<tokio::sync::oneshot::Receiver<Result<SearchConfig, Error>>, Error> {
        let (reply, response) = tokio::sync::oneshot::channel();
        self.sender
            .send(UpdateRequest {
                actor_id,
                config,
                reply,
            })
            .await
            .map_err(|_| Error::internal("search settings worker unavailable"))?;
        Ok(response)
    }

    /// 入队并等待结果；请求取消仅丢弃响应接收端，不会撤回已入队更新。
    pub async fn update(&self, actor_id: i64, config: SearchConfig) -> Result<SearchConfig, Error> {
        self.enqueue(actor_id, config)
            .await?
            .await
            .map_err(|_| Error::internal("search settings worker stopped"))?
    }

    /// 停止接收新请求；worker 会继续排空已经成功入队的更新。
    pub(crate) fn shutdown(&self) {
        if let Some(sender) = self
            .shutdown
            .lock()
            .expect("search settings shutdown mutex poisoned")
            .take()
        {
            let _ = sender.send(());
        }
    }
}

/// 启动搜索设置单写者；返回控制句柄与供主进程监督的任务句柄。
pub fn spawn(
    db: prts_db::Db,
    runtime: Arc<tokio::sync::RwLock<SearchConfig>>,
) -> (SearchSettingsUpdater, tokio::task::JoinHandle<()>) {
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<UpdateRequest>(32);
    let (shutdown, mut shutdown_requested) = tokio::sync::oneshot::channel();
    let updater = SearchSettingsUpdater {
        sender,
        shutdown: Arc::new(std::sync::Mutex::new(Some(shutdown))),
    };
    let handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                request = receiver.recv() => {
                    let Some(request) = request else {
                        break;
                    };
                    process_request(&db, &runtime, request).await;
                }
                _ = &mut shutdown_requested => {
                    receiver.close();
                    while let Some(request) = receiver.recv().await {
                        process_request(&db, &runtime, request).await;
                    }
                    break;
                }
            }
        }
    });
    (updater, handle)
}

async fn process_request(
    db: &prts_db::Db,
    runtime: &tokio::sync::RwLock<SearchConfig>,
    request: UpdateRequest,
) {
    let result = apply_update(db, runtime, request.actor_id, request.config).await;
    let _ = request.reply.send(result);
}

async fn apply_update(
    db: &prts_db::Db,
    runtime: &tokio::sync::RwLock<SearchConfig>,
    actor_id: i64,
    config: SearchConfig,
) -> Result<SearchConfig, Error> {
    let mut tx = db.begin().await.map_err(database_error)?;
    let change = prts_db::search_settings::set_tx(&mut tx, config, Some(actor_id))
        .await
        .map_err(database_error)?;
    let changed_fields = changed_fields(&change.before, &change.after);
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(actor_id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::SearchSettingsUpdated {
            changed_fields: &changed_fields,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(database_error)?;
    let mut runtime_config = runtime.write().await;
    *runtime_config = change.after.clone();
    Ok(change.after)
}

fn changed_fields(before: &SearchConfig, after: &SearchConfig) -> Vec<&'static str> {
    let mut changed = Vec::with_capacity(7);
    if before.embedding_enabled != after.embedding_enabled {
        changed.push("embedding_enabled");
    }
    if before.embedding_model != after.embedding_model {
        changed.push("embedding_model");
    }
    if before.embedding_base_url != after.embedding_base_url {
        changed.push("embedding_base_url");
    }
    if before.embedding_batch != after.embedding_batch {
        changed.push("embedding_batch");
    }
    if before.tm_enabled != after.tm_enabled {
        changed.push("tm_enabled");
    }
    if before.tm_min_similarity != after.tm_min_similarity {
        changed.push("tm_min_similarity");
    }
    if before.tm_top_n != after.tm_top_n {
        changed.push("tm_top_n");
    }
    changed
}

fn database_error(error: sqlx::Error) -> Error {
    Error::internal(format!("db error: {error}"))
}
