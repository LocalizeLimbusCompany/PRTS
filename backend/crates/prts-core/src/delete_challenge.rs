//! 项目删除使用的整数数学 challenge；题目由 typed plan 生成，不解析或执行字符串。

use serde::{Deserialize, Serialize};

/// 管理设置选择的题目难度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeMode {
    Simple,
    Advanced,
}

/// 可直接计算整数答案的封闭题目计划。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChallengePlan {
    Arithmetic {
        left: i64,
        right: i64,
        op: ArithmeticOp,
    },
    DerivativeAt {
        coefficient: i64,
        power: u32,
        point: i64,
    },
    DefiniteIntegralLinear {
        coefficient: i64,
        upper: i64,
    },
    PolynomialLimit {
        coefficient: i64,
        power: u32,
        point: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArithmeticOp {
    Add,
    Subtract,
    Multiply,
}

impl ChallengePlan {
    /// 直接从 typed 字段计算整数答案。
    pub fn answer(&self) -> i64 {
        match *self {
            Self::Arithmetic { left, right, op } => match op {
                ArithmeticOp::Add => left + right,
                ArithmeticOp::Subtract => left - right,
                ArithmeticOp::Multiply => left * right,
            },
            Self::DerivativeAt {
                coefficient,
                power,
                point,
            } => coefficient * i64::from(power) * point.pow(power.saturating_sub(1)),
            Self::DefiniteIntegralLinear { coefficient, upper } => coefficient * upper * upper / 2,
            Self::PolynomialLimit {
                coefficient,
                power,
                point,
            } => coefficient * point.pow(power),
        }
    }

    pub fn prompt(&self) -> String {
        match *self {
            Self::Arithmetic { left, right, op } => {
                let symbol = match op {
                    ArithmeticOp::Add => "+",
                    ArithmeticOp::Subtract => "−",
                    ArithmeticOp::Multiply => "×",
                };
                format!("{left} {symbol} {right} = ?")
            }
            Self::DerivativeAt {
                coefficient,
                power,
                point,
            } => format!("若 f(x) = {coefficient}x^{power}，求 f′({point})。"),
            Self::DefiniteIntegralLinear { coefficient, upper } => {
                format!("计算定积分 ∫₀^{upper} {coefficient}x dx。")
            }
            Self::PolynomialLimit {
                coefficient,
                power,
                point,
            } => format!("计算 lim(x→{point}) {coefficient}x^{power}。"),
        }
    }
}

/// 以服务端随机种子稳定选择模板和有界参数。
pub fn generate_challenge(mode: ChallengeMode, seed: u64) -> ChallengePlan {
    let bounded = |shift: u32, max: u64| ((seed >> shift) % max) as i64 + 1;
    match mode {
        ChallengeMode::Simple => ChallengePlan::Arithmetic {
            left: bounded(0, 30),
            right: bounded(8, 20),
            op: match seed % 3 {
                0 => ArithmeticOp::Add,
                1 => ArithmeticOp::Subtract,
                _ => ArithmeticOp::Multiply,
            },
        },
        ChallengeMode::Advanced => match seed % 3 {
            0 => ChallengePlan::DerivativeAt {
                coefficient: bounded(8, 5),
                power: bounded(16, 3) as u32 + 1,
                point: bounded(24, 4),
            },
            1 => ChallengePlan::DefiniteIntegralLinear {
                coefficient: bounded(8, 4) * 2,
                upper: bounded(16, 5),
            },
            _ => ChallengePlan::PolynomialLimit {
                coefficient: bounded(8, 5),
                power: bounded(16, 3) as u32,
                point: bounded(24, 4),
            },
        },
    }
}

/// 对外 helper，避免调用者重复解释计划。
pub fn answer(plan: &ChallengePlan) -> i64 {
    plan.answer()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_and_advanced_templates_have_integer_answers() {
        for seed in 0..30 {
            let simple = generate_challenge(ChallengeMode::Simple, seed);
            let advanced = generate_challenge(ChallengeMode::Advanced, seed);
            assert_eq!(answer(&simple), simple.answer());
            assert_eq!(answer(&advanced), advanced.answer());
            assert!(!simple.prompt().is_empty());
            assert!(!advanced.prompt().is_empty());
        }
    }
}
