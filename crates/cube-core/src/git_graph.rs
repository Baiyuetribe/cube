//! 提交历史的泳道布局。
//!
//! 把 `git log` 的线性列表算成「每行用哪个车道、车道之间怎么连线」，
//! 渲染层只负责照坐标画。算法是纯函数，可以脱离 UI 单测。
//!
//! 前提：输入按 `--topo-order` 排好，父提交不会出现在子提交之前。

use crate::git::GitCommit;

/// 一条线段：从本行的某车道连向下一行的某车道。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitEdge {
    pub to_lane: usize,
    /// 线段颜色。跟「被连的那个提交」走，不跟车道走——车道会复用，
    /// 跟着车道会让不同分支串成同一个颜色。
    pub color: usize,
}

/// 一行提交的布局结果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitRowLayout {
    /// 本提交圆点所在车道。
    pub lane: usize,
    /// 本提交圆点的颜色索引。
    pub color: usize,
    /// 贯穿本行的车道：这些车道在上一行就已存在，且本行没有提交占据它们，
    /// 需要从行顶画到行底。
    pub pass_through: Vec<(usize, usize)>,
    /// 连向下一行的线段。
    pub edges: Vec<GitEdge>,
}

/// 整个历史的布局。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitGraphLayout {
    pub rows: Vec<GitRowLayout>,
    /// 用到的最大车道数，渲染层据此决定左边留多宽。
    pub max_lanes: usize,
}

/// 一个车道的占用状态。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Lane {
    /// 正在等待的父提交哈希。
    awaiting: Option<String>,
    /// 这条车道代表的分支颜色。
    color: usize,
}

/// 计算泳道布局。
///
/// 每个提交固定占一条车道，直到它的父提交被处理完。连线由「提交 → 它的每个
/// 父提交」直接决定：父提交在哪一行、哪条车道，就连过去。这样多分支并行时
/// 不会出现所有线都往同一侧斜的情况。
pub fn layout(commits: &[GitCommit]) -> GitGraphLayout {
    if commits.is_empty() {
        return GitGraphLayout::default();
    }

    // 先建立 哈希 -> 行号 的索引，连线时据此找到父提交落在哪一行。
    let mut row_of: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::with_capacity(commits.len());
    for (index, commit) in commits.iter().enumerate() {
        row_of.insert(commit.hash.as_str(), index);
    }

    let mut lanes: Vec<Lane> = Vec::new();
    let mut next_color = 0usize;
    let mut lane_of_row: Vec<usize> = Vec::with_capacity(commits.len());
    let mut color_of_row: Vec<usize> = Vec::with_capacity(commits.len());
    // 每行结束时，哪些车道仍然活跃。
    let mut lanes_snapshot: Vec<Vec<(usize, usize)>> = Vec::with_capacity(commits.len());
    let mut max_lanes = 0usize;

    for (row_index, commit) in commits.iter().enumerate() {
        // 找到正在等本提交的车道；没有就新占一条。
        let lane_index = match lanes
            .iter()
            .position(|lane| lane.awaiting.as_deref() == Some(commit.hash.as_str()))
        {
            Some(index) => index,
            None => {
                // 优先复用空车道（最靠左的那个），否则追加到末尾。
                // 不能 insert：那会让后面所有车道的编号整体右移，
                // 已经记录的行坐标全部作废。
                let color = next_color;
                next_color = next_color.wrapping_add(1);
                match lanes.iter().position(|lane| lane.awaiting.is_none()) {
                    Some(index) => {
                        lanes[index].color = color;
                        index
                    }
                    None => {
                        lanes.push(Lane {
                            awaiting: None,
                            color,
                        });
                        lanes.len() - 1
                    }
                }
            }
        };

        let color = lanes[lane_index].color;

        // 本行贯穿车道：其他车道上仍在等待的线。
        //
        // 排除「正在等本提交」的车道——那条线在本行的圆点处就汇入了，
        // 如果继续往下画，就会出现一条凭空延伸到支线里的多余竖线。
        let pass_through: Vec<(usize, usize)> = lanes
            .iter()
            .enumerate()
            .filter(|(index, lane)| {
                *index != lane_index
                    && lane.awaiting.is_some()
                    && lane.awaiting.as_deref() != Some(commit.hash.as_str())
            })
            .map(|(index, lane)| (index, lane.color))
            .collect();

        // 父提交必须排在本行之后（截断的边界处可能是已处理过的行）。
        let live_parent = |parent: &String| {
            row_of
                .get(parent.as_str())
                .is_some_and(|row| *row > row_index)
        };
        let first_parent = commit.parents.first().filter(|p| live_parent(p)).cloned();

        // 本车道是否该在父提交处汇入别的车道。
        //
        // 分两种情况：
        // - 合并提交：第一个父提交若已被别的车道等待，本分支是汇入那条线，
        //   本车道立刻释放（否则车道数随合并次数增长）。
        // - 普通提交：两条分支指向同一祖先时，它们本来就该各自保留车道，
        //   只有自己指向的那条线才继承本车道。
        let merges_into_existing = commit.parents.len() > 1
            && first_parent.as_deref().is_some_and(|parent| {
                lanes.iter().enumerate().any(|(index, lane)| {
                    index != lane_index && lane.awaiting.as_deref() == Some(parent)
                })
            });
        lanes[lane_index].awaiting = if merges_into_existing {
            None
        } else {
            first_parent.clone()
        };

        // 其他车道若也在等本提交，说明那几条线在这里汇入，一并释放。
        // 不清掉的话，那些车道会一直等到历史结束，画面越往下越宽。
        //
        // 释放后立刻回收车道的身份（而不是留着空位等下一步复用），
        // 否则下面给父提交分配车道时会复用刚被清空的那条，让已经结束的
        // 支线看起来凭空延续下去。
        for (index, lane) in lanes.iter_mut().enumerate() {
            if index != lane_index && lane.awaiting.as_deref() == Some(commit.hash.as_str()) {
                lane.awaiting = None;
            }
        }

        // 其余父提交各占一条新车道，紧贴在当前最后一个活跃车道之后。
        for parent in commit.parents.iter().skip(1) {
            if !live_parent(parent) {
                continue;
            }
            if lanes
                .iter()
                .any(|lane| lane.awaiting.as_deref() == Some(parent.as_str()))
            {
                continue;
            }
            let color = next_color;
            next_color = next_color.wrapping_add(1);
            let new_lane = Lane {
                awaiting: Some(parent.clone()),
                color,
            };
            match lanes.iter().position(|lane| lane.awaiting.is_none()) {
                Some(free) => lanes[free] = new_lane,
                None => lanes.push(new_lane),
            }
        }

        // 收掉尾部空车道。
        while lanes.last().is_some_and(|lane| lane.awaiting.is_none()) {
            lanes.pop();
        }

        lane_of_row.push(lane_index);
        color_of_row.push(color);
        lanes_snapshot.push(pass_through);
        max_lanes = max_lanes.max(lanes.len()).max(lane_index + 1);
    }

    // 第二遍：每个提交对它的每个父提交画一条线。
    // 父提交大概率在后续某一行，线要先从本行落到下一行，再由中间各行的
    // pass_through 接力；这里只记录「本行到下一行」的那一段。
    let mut rows: Vec<GitRowLayout> = Vec::with_capacity(commits.len());
    for (index, commit) in commits.iter().enumerate() {
        let lane = lane_of_row[index];
        let color = color_of_row[index];
        let mut edges = Vec::new();

        if index + 1 < commits.len() {
            for (parent_index, parent) in commit.parents.iter().enumerate() {
                let Some(&target_row) = row_of.get(parent.as_str()) else {
                    continue;
                };
                if target_row <= index {
                    // 父提交已经画过（不该发生，防御一下）。
                    continue;
                }
                let target_lane = lane_of_row[target_row];
                let edge_color = if parent_index == 0 {
                    color
                } else {
                    // 合并进来的分支：用目标行自己的颜色，让整条支线同色。
                    color_of_row[target_row]
                };
                // 只在紧邻的下一行内完成换道；父提交在更远的行时，这段线
                // 先竖直落到下一行，之后由中间各行的 pass_through 接力。
                // 这样任何时候都不会出现横跨多行的长斜线。
                let to_lane = if target_row == index + 1 {
                    target_lane
                } else {
                    lane
                };
                edges.push(GitEdge {
                    to_lane,
                    color: edge_color,
                });
            }
        }

        rows.push(GitRowLayout {
            lane,
            color,
            pass_through: lanes_snapshot[index].clone(),
            edges,
        });
    }

    GitGraphLayout { rows, max_lanes }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(hash: &str, parents: &[&str]) -> GitCommit {
        GitCommit {
            hash: hash.into(),
            short_hash: hash[..hash.len().min(7)].into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            author: "t".into(),
            timestamp: 0,
            summary: hash.into(),
            refs: Vec::new(),
        }
    }

    #[test]
    fn linear_history_stays_in_one_lane() {
        let commits = vec![commit("c", &["b"]), commit("b", &["a"]), commit("a", &[])];
        let layout = layout(&commits);

        assert_eq!(layout.max_lanes, 1);
        assert!(layout.rows.iter().all(|row| row.lane == 0));
        assert_eq!(layout.rows[0].edges.len(), 1);
        assert_eq!(layout.rows[0].edges[0].to_lane, 0);
    }

    #[test]
    fn a_merge_opens_a_second_lane() {
        let commits = vec![
            commit("m", &["a", "b"]),
            commit("b", &["r"]),
            commit("a", &["r"]),
            commit("r", &[]),
        ];
        let layout = layout(&commits);

        assert_eq!(layout.rows[0].lane, 0, "合并提交在主车道");
        assert_eq!(layout.rows[1].lane, 1, "第二个父提交占新车道");
        assert_eq!(layout.max_lanes, 2);
    }

    #[test]
    fn merge_lines_converge_at_the_common_ancestor() {
        let commits = vec![
            commit("m", &["a", "b"]),
            commit("b", &["r"]),
            commit("a", &["r"]),
            commit("r", &[]),
        ];
        let layout = layout(&commits);
        assert_eq!(layout.rows.last().unwrap().lane, 0);
        assert!(layout.max_lanes <= 2);
    }

    #[test]
    fn independent_tips_each_get_a_lane() {
        let commits = vec![
            commit("x1", &["x0"]),
            commit("y1", &["y0"]),
            commit("x0", &[]),
            commit("y0", &[]),
        ];
        let layout = layout(&commits);
        assert_eq!(layout.rows[0].lane, 0);
        assert_eq!(layout.rows[1].lane, 1, "独立分支不能用同一个车道");
    }

    #[test]
    fn empty_input_yields_empty_layout() {
        let layout = layout(&[]);
        assert!(layout.rows.is_empty());
        assert_eq!(layout.max_lanes, 0);
    }

    #[test]
    fn root_commit_has_no_outgoing_edge() {
        let layout = layout(&[commit("a", &[])]);
        assert_eq!(layout.rows[0].lane, 0);
        assert!(layout.rows[0].edges.is_empty());
    }

    #[test]
    fn max_lanes_covers_every_row() {
        let commits = vec![
            commit("m", &["a", "b"]),
            commit("b", &["r"]),
            commit("a", &["r"]),
            commit("r", &[]),
        ];
        let layout = layout(&commits);
        for row in &layout.rows {
            assert!(row.lane < layout.max_lanes);
            for (lane, _) in &row.pass_through {
                assert!(*lane < layout.max_lanes);
            }
        }
    }

    #[test]
    fn three_parallel_branches_use_three_lanes() {
        let commits = vec![
            commit("a1", &["base"]),
            commit("b1", &["base"]),
            commit("c1", &["base"]),
            commit("base", &[]),
        ];
        let layout = layout(&commits);

        assert_eq!(layout.max_lanes, 3);
        let lanes: Vec<usize> = layout.rows.iter().take(3).map(|row| row.lane).collect();
        assert_eq!(lanes, vec![0, 1, 2]);
        let colors: Vec<usize> = layout.rows.iter().take(3).map(|row| row.color).collect();
        assert_eq!(
            colors
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            3
        );
    }

    /// 关键回归：分叉期间，每行的连线数不得超过「父提交数」，
    /// 否则就会出现截图里那种所有线都斜过去的扇形。
    #[test]
    fn edges_never_exceed_parent_count() {
        let commits = vec![
            commit("m", &["a", "b"]),
            commit("b", &["r"]),
            commit("a", &["r"]),
            commit("r", &[]),
        ];
        let layout = layout(&commits);
        for (index, row) in layout.rows.iter().enumerate() {
            assert!(
                row.edges.len() <= commits[index].parents.len(),
                "第 {index} 行有 {} 条线，但有 {} 个父提交",
                row.edges.len(),
                commits[index].parents.len()
            );
        }
    }

    /// 贯穿车道的数量不能超过该行实际活跃的分支数，否则会多画线。
    #[test]
    fn pass_through_only_contains_other_active_lanes() {
        let commits = vec![
            commit("m", &["a", "b"]),
            commit("b", &["r"]),
            commit("a", &["r"]),
            commit("r", &[]),
        ];
        let layout = layout(&commits);
        for row in &layout.rows {
            assert!(
                !row.pass_through.iter().any(|(lane, _)| *lane == row.lane),
                "本行车道不该出现在贯穿列表里"
            );
        }
    }
}

#[cfg(test)]
mod continuity_tests {
    use super::*;

    fn commit(hash: &str, parents: &[&str]) -> GitCommit {
        GitCommit {
            hash: hash.into(),
            short_hash: hash.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            author: "t".into(),
            timestamp: 0,
            summary: hash.into(),
            refs: Vec::new(),
        }
    }

    /// 关键不变量：如果第 N 行有一条线连到 lane L，那么第 N+1 行必须有
    /// 东西覆盖 lane L——要么是圆点，要么是 pass_through。否则线会悬空断开。
    fn assert_no_dangling_edges(commits: &[GitCommit], layout: &GitGraphLayout) {
        for (index, row) in layout.rows.iter().enumerate() {
            let Some(next) = layout.rows.get(index + 1) else {
                continue;
            };
            for edge in &row.edges {
                let covered = next.lane == edge.to_lane
                    || next
                        .pass_through
                        .iter()
                        .any(|(lane, _)| *lane == edge.to_lane);
                assert!(
                    covered,
                    "第 {index} 行（{}）的线连到第 {} 行的 lane {}，但那里没有圆点也没有贯穿线",
                    commits[index].hash,
                    index + 1,
                    edge.to_lane
                );
            }
        }
    }

    #[test]
    fn merge_history_has_no_dangling_edges() {
        let commits = vec![
            commit("m", &["a", "b"]),
            commit("b", &["r"]),
            commit("a", &["r"]),
            commit("r", &[]),
        ];
        assert_no_dangling_edges(&commits, &layout(&commits));
    }

    /// 长链 + 交错合并，覆盖「父提交在很远的行」的情况。
    #[test]
    fn interleaved_merges_have_no_dangling_edges() {
        let commits = vec![
            commit("m1", &["a1", "b1"]),
            commit("b1", &["b0"]),
            commit("a1", &["a0"]),
            commit("m2", &["a0", "c1"]),
            commit("c1", &["c0"]),
            commit("a0", &["base"]),
            commit("c0", &["base"]),
            commit("base", &[]),
        ];
        assert_no_dangling_edges(&commits, &layout(&commits));
    }

    /// 乱序输入（父提交可能在列表里找不到）不能 panic。
    #[test]
    fn missing_parents_do_not_panic() {
        let commits = vec![
            commit("a", &["not-in-list"]),
            commit("b", &["also-missing"]),
        ];
        let layout = layout(&commits);
        assert_eq!(layout.rows.len(), 2);
    }
}
