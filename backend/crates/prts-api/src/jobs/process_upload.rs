//! 上传业务文件处理器的阶段边界。
//!
//! Task 3.1 只建立可靠传输与持久队列；完整替换规则由 Task 3.2 注册真实 handler。

/// `upload_process` 在完整替换服务发布前保持未注册，queued job 不会被未知 worker 领取。
pub const KIND: &str = "upload_process";
