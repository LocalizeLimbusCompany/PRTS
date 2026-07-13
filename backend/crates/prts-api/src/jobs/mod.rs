//! 可扩展任务 handler 注册表。
//!
//! foundation 阶段不提前实现后续上传/重建/purge 业务 handler；worker 只会领取已注册
//! kind，因此空注册表是安全的 dormant 状态，后续阶段可逐项注册执行器。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use futures_util::future::BoxFuture;

pub use prts_db::jobs::JobResult;

/// foundation worker 当前实际产生的稳定错误码；后续 handler 按需扩展。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobErrorCode {
    HandlerUnavailable,
    InvalidPayload,
    DatabaseUnavailable,
    LanguageResolutionRequired,
    UploadTempUnavailable,
    UploadInvalidJson,
    UploadInvalidEntry,
    UploadDuplicateKey,
    UploadInvalidLanguage,
    UploadSourceLanguageMismatch,
}

impl JobErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HandlerUnavailable => "job_handler_unavailable",
            Self::InvalidPayload => "job_invalid_payload",
            Self::DatabaseUnavailable => "job_database_unavailable",
            Self::LanguageResolutionRequired => "project_language_resolution_required",
            Self::UploadTempUnavailable => "upload_temp_unavailable",
            Self::UploadInvalidJson => "upload_invalid_json",
            Self::UploadInvalidEntry => "upload_invalid_entry",
            Self::UploadDuplicateKey => "upload_duplicate_key",
            Self::UploadInvalidLanguage => "upload_invalid_language",
            Self::UploadSourceLanguageMismatch => "upload_source_language_mismatch",
        }
    }

    pub const fn redacted_message(self) -> &'static str {
        match self {
            Self::HandlerUnavailable => "job handler is not available",
            Self::InvalidPayload => "job payload is invalid",
            Self::DatabaseUnavailable => "job database operation failed",
            Self::LanguageResolutionRequired => "project language resolution is required",
            Self::UploadTempUnavailable => "upload temp file is unavailable",
            Self::UploadInvalidJson => "upload JSON is invalid",
            Self::UploadInvalidEntry => "upload entry is invalid",
            Self::UploadDuplicateKey => "upload contains a duplicate entry key",
            Self::UploadInvalidLanguage => "upload contains an invalid language tag",
            Self::UploadSourceLanguageMismatch => {
                "upload language is not configured for the project"
            }
        }
    }
}

pub mod cleanup_uploads;
pub mod process_upload;
pub mod reindex_project;
pub mod repair_languages;

/// 任务执行失败的稳定信息。
#[derive(Debug, Clone)]
pub struct JobExecutionError {
    pub code: JobErrorCode,
    /// handler 原始错误只用于进程内诊断，不得持久化或进入 API。
    pub message: String,
    pub retryable: bool,
    /// 可持久化的 allowlisted 位置元数据；不得包含源文、译文或原始 parser 文本。
    pub details: Option<serde_json::Value>,
}

/// 单一任务 kind 的执行器。
pub trait JobHandler: Send + Sync {
    /// 与 `jobs.kind` 对应的稳定标识。
    fn kind(&self) -> &'static str;

    /// 执行已租用任务。handler 通过仓储更新阶段/进度，不直接操作路由状态。
    fn execute<'a>(
        &'a self,
        job: &'a prts_db::models::Job,
    ) -> BoxFuture<'a, Result<JobResult, JobExecutionError>>;
}

/// 线程安全的 handler 注册表。
#[derive(Clone)]
pub struct JobRegistry {
    handlers: Arc<RwLock<HashMap<String, Arc<dyn JobHandler>>>>,
}

impl JobRegistry {
    /// 从当前 release 提供的 handler 构造注册表；空列表表示安全 dormant worker。
    pub fn new(handlers: Vec<Arc<dyn JobHandler>>) -> Self {
        let handlers = handlers
            .into_iter()
            .map(|handler| (handler.kind().to_string(), handler))
            .collect();
        Self {
            handlers: Arc::new(RwLock::new(handlers)),
        }
    }

    /// 当前可安全领取的任务 kind。
    pub fn kinds(&self) -> Vec<String> {
        self.handlers
            .read()
            .expect("job registry lock poisoned")
            .keys()
            .cloned()
            .collect()
    }

    /// 获取执行器快照。
    pub fn get(&self, kind: &str) -> Option<Arc<dyn JobHandler>> {
        self.handlers
            .read()
            .expect("job registry lock poisoned")
            .get(kind)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TypedResultHandler;

    impl JobHandler for TypedResultHandler {
        fn kind(&self) -> &'static str {
            "upload_cleanup"
        }

        fn execute<'a>(
            &'a self,
            _job: &'a prts_db::models::Job,
        ) -> BoxFuture<'a, Result<JobResult, JobExecutionError>> {
            Box::pin(async { Ok(JobResult::Completed) })
        }
    }

    #[test]
    fn handlers_return_allowlisted_results_instead_of_arbitrary_json() {
        let registry = JobRegistry::new(vec![Arc::new(TypedResultHandler)]);
        assert!(registry.get("upload_cleanup").is_some());
    }
}
