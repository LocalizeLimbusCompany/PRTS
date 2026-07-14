//! `prts-core` —— 领域逻辑（项目 / 文件 / 词条 / 状态机 / CP / 权限 / 历史）。
//!
//! P0 仅落地最基础、跨阶段复用的领域类型（词条状态与标志位），以锁定线上线格式（wire format）；
//! 具体业务实现见后续阶段：P2 文件系统、P3 编辑器、P6 CP。
//!
//! 设计要点（见 plan §6）：
//! - 翻译工作流是单值枚举 [`EntryState`]；
//! - `locked` / `hidden` 是**正交标志位**（[`EntryFlags`]），独立于工作流。

pub mod capabilities;
pub mod delete_challenge;
pub mod entry;
pub mod file_history;
pub mod jobs;
pub mod language;
pub mod permission;
pub mod ports;
pub mod project_language;
pub mod search_query;
pub mod tasks;
pub mod terms;
pub mod upload_replacement;

pub use entry::{EntryFlags, EntryState};
pub use jobs::JobState;
pub use language::{canonicalize_language_tag, canonicalize_language_tags, LanguageTagError};
pub use permission::{PlatformRole, ProjectRole};
