//! 词条状态与标志位的领域类型。

use serde::{Deserialize, Serialize};

/// 词条翻译工作流状态（单值）。
///
/// 线上线格式（上传/下载/API）使用小写英文标识，见各变体的 `rename`。
/// `locked` / `hidden` **不属于**本枚举，见 [`EntryFlags`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    /// 未翻译。
    #[default]
    Untranslated,
    /// 已翻译。
    Translated,
    /// 已检查。
    Checked,
    /// 已审核。
    Reviewed,
}

impl EntryState {
    /// 解析线上线标识。
    pub fn parse(s: &str) -> Option<EntryState> {
        match s {
            "untranslated" => Some(Self::Untranslated),
            "translated" => Some(Self::Translated),
            "checked" => Some(Self::Checked),
            "reviewed" => Some(Self::Reviewed),
            _ => None,
        }
    }

    /// 稳定字符串标识。
    pub fn as_str(&self) -> &'static str {
        match self {
            EntryState::Untranslated => "untranslated",
            EntryState::Translated => "translated",
            EntryState::Checked => "checked",
            EntryState::Reviewed => "reviewed",
        }
    }
}

/// 独立于工作流的正交标志位。
///
/// - `locked`：锁定后仅项目「管理」与「拥有者」可修改该词条（见 plan §6、§7）。
/// - `hidden`：从常规列表 / 翻译视图中隐藏。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EntryFlags {
    /// 已锁定。
    #[serde(default)]
    pub locked: bool,
    /// 已隐藏。
    #[serde(default)]
    pub hidden: bool,
    /// 有疑问标签；独立于翻译工作流。
    #[serde(default)]
    pub questioned: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_serializes_to_wire_identifiers() {
        assert_eq!(
            serde_json::to_string(&EntryState::Untranslated).unwrap(),
            "\"untranslated\""
        );
        assert_eq!(
            serde_json::to_string(&EntryState::Reviewed).unwrap(),
            "\"reviewed\""
        );
        let s: EntryState = serde_json::from_str("\"checked\"").unwrap();
        assert_eq!(s, EntryState::Checked);
    }

    #[test]
    fn default_state_is_untranslated_and_flags_false() {
        assert_eq!(EntryState::default(), EntryState::Untranslated);
        let f = EntryFlags::default();
        assert!(!f.locked && !f.hidden && !f.questioned);
    }
}
