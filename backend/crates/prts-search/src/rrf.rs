//! Reciprocal Rank Fusion：合并多路有序召回为单一排序。
use std::collections::HashMap;

const RRF_K: f64 = 60.0;

/// 一路召回：按相关度降序的 entry id。
pub type RankedIds = Vec<i64>;

/// 融合多路结果，返回按融合分降序的 (id, score)；并列按 id 升序（确定性）。
pub fn rrf_fuse(paths: &[RankedIds]) -> Vec<(i64, f64)> {
    let mut scores: HashMap<i64, f64> = HashMap::new();
    for path in paths {
        for (rank, &id) in path.iter().enumerate() {
            *scores.entry(id).or_insert(0.0) += 1.0 / (RRF_K + rank as f64 + 1.0);
        }
    }
    let mut fused: Vec<(i64, f64)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then(a.0.cmp(&b.0)));
    fused
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_in_two_paths_outranks_singletons() {
        let out = rrf_fuse(&[vec![7, 3, 1], vec![7, 9]]);
        assert_eq!(out[0].0, 7);
        assert!(out[0].1 > out[1].1);
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(rrf_fuse(&[]).is_empty());
    }

    #[test]
    fn ties_break_by_id_ascending() {
        let out = rrf_fuse(&[vec![5], vec![2]]); // 同分
        assert_eq!(out[0].0, 2);
        assert_eq!(out[1].0, 5);
    }
}
