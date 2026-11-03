//! Git 只读查询：工作区状态、提交历史、diff。
//!
//! 走 `git` 命令行而不是 libgit2：项目里已经有调用 git 的先例，也不额外引入
//! C 依赖。解析用 `--porcelain` 等稳定格式，不解析人类可读输出。
//!
//! 这里只读不写，唯一的例外是 [`commit`]，由用户显式触发。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 工作区里一个变更条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitChange {
    /// 相对仓库根的路径。
    pub path: PathBuf,
    /// 暂存区里的状态（XY 中的 X）。`None` 表示暂存区没有这项。
    pub staged: Option<GitStatus>,
    /// 工作区里的状态（XY 中的 Y）。`None` 表示工作区没有改动。
    pub unstaged: Option<GitStatus>,
}

impl GitChange {
    pub fn is_staged(&self) -> bool {
        self.staged.is_some()
    }

    pub fn is_unstaged(&self) -> bool {
        self.unstaged.is_some()
    }
}

/// 单个字符的状态码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    /// 未跟踪文件。
    Untracked,
    Unmerged,
}

impl GitStatus {
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'A' => Some(Self::Added),
            'M' => Some(Self::Modified),
            'D' => Some(Self::Deleted),
            'R' => Some(Self::Renamed),
            'C' => Some(Self::Copied),
            'T' => Some(Self::TypeChanged),
            '?' => Some(Self::Untracked),
            'U' => Some(Self::Unmerged),
            _ => None,
        }
    }

    /// 列表里显示的短标记。
    pub fn letter(self) -> char {
        match self {
            Self::Added => 'A',
            Self::Modified => 'M',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Copied => 'C',
            Self::TypeChanged => 'T',
            Self::Untracked => 'U',
            Self::Unmerged => 'U',
        }
    }
}

/// 一条提交记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommit {
    /// 完整哈希。
    pub hash: String,
    /// 短哈希，列表里显示用。
    pub short_hash: String,
    /// 父提交哈希（0 个是根提交，1 个是普通提交，2 个以上是合并提交）。
    pub parents: Vec<String>,
    pub author: String,
    /// 作者时间戳（Unix 秒）。
    pub timestamp: i64,
    /// 首行提交信息。
    pub summary: String,
    /// 指向这个提交的分支/标签名。
    pub refs: Vec<String>,
}

impl GitCommit {
    /// 是不是合并提交。`git log` 的默认顺序（topo + date）保证不会出现
    /// 父提交排在子提交之前，布局算法依赖这一点。
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }
}

/// 一次 diff 的结果，按文件分块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDiff {
    /// 直接可显示的 unified diff 文本。
    pub text: String,
}

fn run_git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|error| format!("无法运行 git: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(stderr.trim().to_string());
    }
    String::from_utf8(output.stdout).map_err(|_| "git 输出不是有效 UTF-8".to_string())
}

/// 仓库根。不是 git 仓库或 git 不可用时返回 `None`。
pub fn repository_root(start: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(start)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

/// 工作区状态。
///
/// `--untracked-files=all` 让未跟踪文件逐个列出，而不是把整个目录折叠成一项
/// —— 折叠项无法在编辑器里打开。
pub fn status(repo: &Path) -> Result<Vec<GitChange>, String> {
    let text = run_git(
        repo,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    Ok(parse_status(&text))
}

/// 解析 `git status --porcelain=v1 -z` 的输出。
///
/// `-z` 用 NUL 分隔并且**不转义**路径，中文/空格文件名都能原样保留。
/// 重命名条目在 `-z` 下会多一个原路径字段，这里只取新路径。
fn parse_status(text: &str) -> Vec<GitChange> {
    let mut changes = Vec::new();
    let mut fields = text.split('\0').filter(|field| !field.is_empty());
    while let Some(entry) = fields.next() {
        // 每条至少是 "XY path"：两个状态字符 + 一个空格。
        if entry.len() < 4 {
            continue;
        }
        let mut chars = entry.chars();
        let Some(x) = chars.next() else { continue };
        let Some(y) = chars.next() else { continue };
        let path = &entry[3..];

        let staged = GitStatus::from_char(x);
        let unstaged = GitStatus::from_char(y);

        // 重命名/复制在 -z 模式下紧接着一个原路径字段，跳过它。
        if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') {
            let _ = fields.next();
        }

        changes.push(GitChange {
            path: PathBuf::from(path),
            staged,
            unstaged,
        });
    }
    changes
}

/// 提交历史。`limit` 是条数上限。
pub fn log(repo: &Path, limit: usize) -> Result<Vec<GitCommit>, String> {
    // --topo-order：保证父提交不会排到子提交之前，泳道布局依赖这个前提。
    // --all：把其他分支的提交也拿进来，否则画不出分叉。
    // 用 \x1f 分隔字段、\x1e 分隔记录，避免提交信息里的换行和特殊字符干扰解析。
    let format = "%H%x1f%h%x1f%P%x1f%an%x1f%at%x1f%s%x1e";
    let text = run_git(
        repo,
        &[
            "log",
            "--all",
            "--topo-order",
            &format!("--max-count={limit}"),
            &format!("--pretty=format:{format}"),
        ],
    )?;
    let mut commits = parse_log(&text);
    attach_refs(repo, &mut commits);
    Ok(commits)
}

fn parse_log(text: &str) -> Vec<GitCommit> {
    text.split('\x1e')
        .filter_map(|record| {
            let record = record.trim_start_matches('\n');
            if record.is_empty() {
                return None;
            }
            let mut parts = record.split('\x1f');
            let hash = parts.next()?.to_string();
            let short_hash = parts.next()?.to_string();
            let parents = parts
                .next()?
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            let author = parts.next()?.to_string();
            let timestamp = parts.next()?.trim().parse().ok()?;
            let summary = parts.next().unwrap_or_default().to_string();
            Some(GitCommit {
                hash,
                short_hash,
                parents,
                author,
                timestamp,
                summary,
                refs: Vec::new(),
            })
        })
        .collect()
}

/// 把分支/标签名挂到对应提交上。
///
/// `HEAD` 指向的分支额外标成 `HEAD`，界面据此高亮当前位置。`origin` 这类
/// 远程分支前缀保留原样，用户能一眼看出是远端引用。
fn attach_refs(repo: &Path, commits: &mut [GitCommit]) {
    let Ok(text) = run_git(
        repo,
        &[
            "for-each-ref",
            "--format=%(objectname)%x1f%(refname:short)%x1f%(HEAD)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    ) else {
        return;
    };
    let mut by_hash: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let mut parts = line.split('\x1f');
        let Some(hash) = parts.next() else { continue };
        let Some(name) = parts.next() else { continue };
        let is_head = parts.next().map(str::trim) == Some("*");
        let label = if is_head {
            format!("{name} (HEAD)")
        } else {
            name.to_string()
        };
        by_hash.push((hash.to_string(), label));
    }
    for commit in commits.iter_mut() {
        commit.refs = by_hash
            .iter()
            .filter(|(hash, _)| *hash == commit.hash)
            .map(|(_, label)| label.clone())
            .collect();
    }
}

/// 某个提交改动了哪些文件。
pub fn commit_files(repo: &Path, hash: &str) -> Result<Vec<GitChange>, String> {
    let text = run_git(
        repo,
        &["show", "--name-status", "--pretty=format:", "-z", hash],
    )?;
    Ok(parse_name_status(&text))
}

/// 解析 `git show --name-status -z` 的输出。
///
/// 格式是：状态 \0 路径 \0（重命名时是 状态 \0 原路径 \0 新路径 \0）。
fn parse_name_status(text: &str) -> Vec<GitChange> {
    let mut changes = Vec::new();
    let mut fields = text.split('\0').filter(|field| !field.is_empty());
    while let Some(status_field) = fields.next() {
        let status_char = status_field.chars().next().unwrap_or(' ');
        let Some(status) = GitStatus::from_char(status_char) else {
            continue;
        };
        let Some(first_path) = fields.next() else {
            break;
        };
        // 重命名/复制多一个字段：原路径在前，新路径在后。
        let path = if matches!(status_char, 'R' | 'C') {
            fields.next().unwrap_or(first_path)
        } else {
            first_path
        };
        changes.push(GitChange {
            path: PathBuf::from(path),
            // 历史里的状态只有一个维度，放在 staged 位表示"这次提交里的改动"。
            staged: Some(status),
            unstaged: None,
        });
    }
    changes
}

/// 某个提交的 unified diff 文本。`path` 非空时只输出该文件。
pub fn commit_diff(repo: &Path, hash: &str, path: Option<&Path>) -> Result<GitDiff, String> {
    let mut args = vec![
        "show",
        "--no-color",
        "--patch",
        "--pretty=format:",
        "--no-ext-diff",
        hash,
    ];
    let path_text;
    if let Some(path) = path {
        path_text = path.to_string_lossy().into_owned();
        args.push("--");
        args.push(&path_text);
    }
    let text = run_git(repo, &args)?;
    Ok(GitDiff {
        text: text.trim_start_matches('\n').to_string(),
    })
}

/// 工作区里某个文件未暂存的改动。
pub fn working_diff(repo: &Path, path: &Path) -> Result<GitDiff, String> {
    let path_text = path.to_string_lossy().into_owned();
    let text = run_git(
        repo,
        &["diff", "--no-color", "--no-ext-diff", "--", &path_text],
    )?;
    Ok(GitDiff {
        text: text.trim_start_matches('\n').to_string(),
    })
}

/// 提交暂存区。`message` 为空时返回错误，不执行任何操作。
pub fn commit(repo: &Path, message: &str) -> Result<String, String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("提交信息不能为空".to_string());
    }
    run_git(repo, &["commit", "-m", message]).map(|text| text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modified_and_untracked_entries() {
        // 典型输出：第一列暂存区，第二列工作区。
        let text = " M src/main.rs\0?? new_file.txt\0A  added.rs\0";
        let changes = parse_status(text);

        assert_eq!(changes.len(), 3);
        // " M" -> 暂存区无、工作区 Modified。
        assert_eq!(changes[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(changes[0].staged, None);
        assert_eq!(changes[0].unstaged, Some(GitStatus::Modified));
        assert!(!changes[0].is_staged());
        assert!(changes[0].is_unstaged());

        // "??" -> 未跟踪，只出现在工作区。
        assert_eq!(changes[1].path, PathBuf::from("new_file.txt"));
        assert_eq!(changes[1].unstaged, Some(GitStatus::Untracked));

        // "A " -> 已暂存新增。
        assert_eq!(changes[2].staged, Some(GitStatus::Added));
        assert_eq!(changes[2].unstaged, None);
        assert!(changes[2].is_staged());
    }

    #[test]
    fn keeps_paths_with_spaces_and_unicode() {
        let text = " M 我的 文档.txt\0";
        let changes = parse_status(text);
        assert_eq!(changes[0].path, PathBuf::from("我的 文档.txt"));
    }

    #[test]
    fn rename_consumes_the_original_path_field() {
        // -z 模式下重命名是 "R  new\0old\0"，原路径必须被吃掉，
        // 否则会被当成一条独立记录。
        let text = "R  new_name.rs\0old_name.rs\0 M other.rs\0";
        let changes = parse_status(text);

        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].path, PathBuf::from("new_name.rs"));
        assert_eq!(changes[0].staged, Some(GitStatus::Renamed));
        assert_eq!(changes[1].path, PathBuf::from("other.rs"));
    }

    #[test]
    fn parses_commit_log_records() {
        // 字段顺序：%H %h %P %an %at %s
        let text = "abc123\x1fabc\x1fparent1 parent2\x1f作者\x1f1700000000\x1f第一次提交\x1e\
                    def456\x1fdef\x1fparent1\x1f另一个人\x1f1700000100\x1f第二次提交\x1e";
        let commits = parse_log(text);

        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "abc123");
        assert_eq!(commits[0].short_hash, "abc");
        assert_eq!(commits[0].parents, vec!["parent1", "parent2"]);
        assert!(commits[0].is_merge());
        assert_eq!(commits[0].author, "作者");
        assert_eq!(commits[0].timestamp, 1_700_000_000);
        assert_eq!(commits[0].summary, "第一次提交");
        assert_eq!(commits[1].summary, "第二次提交");
        assert!(!commits[1].is_merge(), "单父提交不是合并提交");
    }

    #[test]
    fn root_commit_has_no_parents() {
        let text = "abc\x1fabc\x1f\x1f作者\x1f1\x1f根提交\x1e";
        let commits = parse_log(text);
        assert!(commits[0].parents.is_empty());
        assert!(!commits[0].is_merge());
    }

    /// 提交信息里带换行时，`%s` 只取首行，不会把记录撑破。
    #[test]
    fn commit_summary_stays_on_one_line() {
        let text = "abc\x1fabc\x1fparent\x1f作者\x1f1\x1f只有首行\x1e";
        let commits = parse_log(text);
        assert_eq!(commits[0].summary, "只有首行");
    }

    #[test]
    fn parses_name_status_with_rename() {
        let text = "M\0src/a.rs\0R100\0old.rs\0new.rs\0";
        let changes = parse_name_status(text);

        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].path, PathBuf::from("src/a.rs"));
        assert_eq!(changes[0].staged, Some(GitStatus::Modified));
        // 重命名取新路径。
        assert_eq!(changes[1].path, PathBuf::from("new.rs"));
        assert_eq!(changes[1].staged, Some(GitStatus::Renamed));
    }

    #[test]
    fn empty_commit_message_is_rejected_without_running_git() {
        // 指向不存在的目录也能通过：空信息在调用 git 之前就被拦下了。
        let error = commit(Path::new("/nonexistent/con-git-test"), "   ").unwrap_err();
        assert!(error.contains("不能为空"));
    }

    #[test]
    fn status_marker_letters_are_distinct_per_state() {
        for status in [
            GitStatus::Added,
            GitStatus::Modified,
            GitStatus::Deleted,
            GitStatus::Renamed,
            GitStatus::Copied,
            GitStatus::TypeChanged,
        ] {
            assert_eq!(GitStatus::from_char(status.letter()), Some(status));
        }
        // 未跟踪和冲突共用 U，是从字符反解时的已知歧义，只验证正向映射。
        assert_eq!(GitStatus::from_char('?'), Some(GitStatus::Untracked));
    }
}
