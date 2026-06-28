//! `prts-common` —— PRTS 跨 crate 共享的基础设施。
//!
//! 包含：分层配置加载（[`config`]）、统一错误类型（[`error`]）、
//! 日志初始化（[`logging`]）、轻量国际化（[`i18n`]）。
//!
//! 本 crate 不含任何业务逻辑，供其余 crate 依赖。

pub mod config;
pub mod error;
pub mod i18n;
pub mod logging;

pub use error::{Error, Result};
