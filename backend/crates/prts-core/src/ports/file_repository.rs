//! 文件 replacement 持久化端口。
//!
//! 端口只接受 [`ReplacementPlan`]，因此具体 PostgreSQL adapter 不能接收 raw upload
//! 后自行重判 insert/restore/source-change/tombstone。大文件实现可多次提交 bounded plan
//! 到同一事务 temp table，最后由 adapter 一次集合应用。

use std::future::Future;

use crate::file_history::FileHistoryPlan;
use crate::upload_replacement::ReplacementPlan;

/// typed replacement plan 的基础设施 sink。
pub trait FileRepository {
    /// adapter 错误类型。
    type Error;

    /// 把一个 bounded typed plan staging 到当前文件事务。
    fn stage_replacement_plan<'a>(
        &'a mut self,
        plan: &'a ReplacementPlan,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a;

    /// 在调用方事务内执行一份已经完成领域校验的文件历史计划。
    ///
    /// adapter 只能按 typed mutations 与 history deltas 执行，不得根据 raw rows
    /// 重新判断 move/delete/restore/rollback 的路径、ownership 或统计规则。
    fn apply_file_history_plan<'a>(
        &'a mut self,
        plan: &'a FileHistoryPlan,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a;
}
