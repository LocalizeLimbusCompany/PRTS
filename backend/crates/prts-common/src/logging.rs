//! 日志/追踪初始化。
//!
//! 通过 `RUST_LOG` 控制级别（缺省 `info`）。可经 `PRTS_LOG_FORMAT=json` 切换为 JSON 输出，
//! 便于生产环境采集。

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// 初始化全局 tracing 订阅者。重复调用安全（忽略二次初始化错误）。
pub fn init() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));

    let json = std::env::var("PRTS_LOG_FORMAT")
        .map(|v| v.eq_ignore_ascii_case("json"))
        .unwrap_or(false);

    let registry = tracing_subscriber::registry().with(filter);

    if json {
        let _ = registry.with(fmt::layer().json()).try_init();
    } else {
        let _ = registry.with(fmt::layer()).try_init();
    }
}
