//! 贡献分（CP）的唯一领域规则。
//!
//! CP 使用十分之一整数保存：普通翻译/编辑权重为 1.0（每个距离单位 10），
//! 校对/审核权重为 0.3（每个距离单位 3）。上传、回滚、恢复和系统任务不调用本模块。

use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};

use crate::EntryState;

/// 一次在线词条保存的计分类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContributionKind {
    /// 翻译或普通编辑。
    Edit,
    /// 保存到已检查或已审核状态。
    Review,
}

impl ContributionKind {
    /// 由服务端已校验的目标状态决定权重。
    pub const fn for_target_state(state: EntryState) -> Self {
        match state {
            EntryState::Checked | EntryState::Reviewed => Self::Review,
            EntryState::Untranslated | EntryState::Translated | EntryState::Questioned => {
                Self::Edit
            }
        }
    }

    /// 稳定数据库 wire value。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::Review => "review",
        }
    }

    const fn weight_tenths(self) -> i64 {
        match self {
            Self::Edit => 10,
            Self::Review => 3,
        }
    }
}

/// CP 计算失败只可能来自不可表示的极端字符串长度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContributionOverflow;

/// 一次保存的可持久化精确计分结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContributionAward {
    pub kind: ContributionKind,
    pub distance: i64,
    pub cp_tenths: i64,
}

/// 按 Unicode 标量值计算 Levenshtein 距离并应用精确十分之一权重。
pub fn calculate_contribution(
    previous_translation: &str,
    new_translation: &str,
    kind: ContributionKind,
) -> Result<ContributionAward, ContributionOverflow> {
    let distance = levenshtein(previous_translation, new_translation);
    let distance = i64::try_from(distance).map_err(|_| ContributionOverflow)?;
    let cp_tenths = distance
        .checked_mul(kind.weight_tenths())
        .ok_or(ContributionOverflow)?;
    Ok(ContributionAward {
        kind,
        distance,
        cp_tenths,
    })
}

/// 平台排行榜周期。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderboardPeriod {
    /// 全部历史，直接读取用户累计值。
    All,
    /// UTC 自然月。
    Month,
    /// UTC 自然周（周一 00:00 开始）。
    Week,
}

impl LeaderboardPeriod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Month => "month",
            Self::Week => "week",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "month" => Some(Self::Month),
            "week" => Some(Self::Week),
            _ => None,
        }
    }

    /// 返回 `[start, end)` UTC 边界；累计榜没有时间边界。
    pub fn bounds(self, now: DateTime<Utc>) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        match self {
            Self::All => None,
            Self::Month => {
                let start = Utc
                    .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
                    .single()
                    .expect("valid UTC month boundary");
                let (next_year, next_month) = if now.month() == 12 {
                    (now.year() + 1, 1)
                } else {
                    (now.year(), now.month() + 1)
                };
                let end = Utc
                    .with_ymd_and_hms(next_year, next_month, 1, 0, 0, 0)
                    .single()
                    .expect("valid UTC month boundary");
                Some((start, end))
            }
            Self::Week => {
                let midnight = Utc
                    .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
                    .single()
                    .expect("valid UTC day boundary");
                let start =
                    midnight - Duration::days(i64::from(now.weekday().num_days_from_monday()));
                Some((start, start + Duration::days(7)))
            }
        }
    }
}

fn levenshtein(left: &str, right: &str) -> usize {
    let left_len = left.chars().count();
    let right_len = right.chars().count();
    let (shorter, longer, shorter_len) = if left_len <= right_len {
        (left, right, left_len)
    } else {
        (right, left, right_len)
    };
    if shorter_len == 0 {
        return longer.chars().count();
    }

    let shorter: Vec<char> = shorter.chars().collect();
    let mut previous: Vec<usize> = (0..=shorter_len).collect();
    let mut current = vec![0; shorter_len + 1];
    for (row, longer_char) in longer.chars().enumerate() {
        current[0] = row + 1;
        for (column, shorter_char) in shorter.iter().enumerate() {
            let substitution = previous[column] + usize::from(*shorter_char != longer_char);
            let insertion = current[column] + 1;
            let deletion = previous[column + 1] + 1;
            current[column + 1] = substitution.min(insertion).min(deletion);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[shorter_len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_and_review_weights_use_exact_tenths() {
        assert_eq!(
            calculate_contribution("", "翻译", ContributionKind::Edit),
            Ok(ContributionAward {
                kind: ContributionKind::Edit,
                distance: 2,
                cp_tenths: 20,
            })
        );
        assert_eq!(
            calculate_contribution("", "翻译", ContributionKind::Review)
                .unwrap()
                .cp_tenths,
            6
        );
        assert_eq!(
            calculate_contribution("same", "same", ContributionKind::Edit)
                .unwrap()
                .cp_tenths,
            0
        );
        assert_eq!(
            calculate_contribution("kitten", "sitting", ContributionKind::Edit)
                .unwrap()
                .cp_tenths,
            30
        );
    }

    #[test]
    fn target_state_selects_review_weight_only_for_checked_and_reviewed() {
        assert_eq!(
            ContributionKind::for_target_state(EntryState::Translated),
            ContributionKind::Edit
        );
        assert_eq!(
            ContributionKind::for_target_state(EntryState::Questioned),
            ContributionKind::Edit
        );
        assert_eq!(
            ContributionKind::for_target_state(EntryState::Checked),
            ContributionKind::Review
        );
        assert_eq!(
            ContributionKind::for_target_state(EntryState::Reviewed),
            ContributionKind::Review
        );
    }

    #[test]
    fn utc_month_and_monday_week_boundaries_are_explicit() {
        let now = Utc
            .with_ymd_and_hms(2026, 7, 15, 12, 30, 0)
            .single()
            .unwrap();
        let (month_start, month_end) = LeaderboardPeriod::Month.bounds(now).unwrap();
        assert_eq!(month_start.to_rfc3339(), "2026-07-01T00:00:00+00:00");
        assert_eq!(month_end.to_rfc3339(), "2026-08-01T00:00:00+00:00");
        let (week_start, week_end) = LeaderboardPeriod::Week.bounds(now).unwrap();
        assert_eq!(week_start.to_rfc3339(), "2026-07-13T00:00:00+00:00");
        assert_eq!(week_end.to_rfc3339(), "2026-07-20T00:00:00+00:00");
    }
}
