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
    /// 重命名/复制时的原路径。其余情况为 `None`。
    pub old_path: Option<PathBuf>,
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

/// unified diff 里一行的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    /// 文件头（`diff --git`、`index`、`---`、`+++`）。
    FileHeader,
    /// hunk 头 `@@ -a,b +c,d @@`。
    HunkHeader,
    /// 未改动的上下文行。
    Context,
    /// 新增行。
    Added,
    /// 删除行。
    Removed,
    /// `\ No newline at end of file` 之类的标记。
    Marker,
    /// 二进制文件占位行（`Binary files ... differ`、`GIT binary patch` 及其数据）。
    Binary,
}

/// diff 里的一行，带新旧行号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    /// 旧文件里的行号（删除行和上下文行有）。
    pub old_line: Option<u32>,
    /// 新文件里的行号（新增行和上下文行有）。
    pub new_line: Option<u32>,
    /// 去掉前缀符号后的正文。
    pub text: String,
}

/// 行内差异的一段：`start..end` 字节范围被改动。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InlineSpan {
    pub start: usize,
    pub end: usize,
}

/// 算两行之间的字符级差异，返回两侧各自需要高亮的字节区间。
///
/// 用最长公共子序列定位相同的片段，剩下的就是改动部分。diff 的一对被
/// 删/增行通常很短，O(n·m) 的动态规划完全够用。
///
/// 返回 `(removed_spans, added_spans)`。
pub fn inline_diff(old: &str, new: &str) -> (Vec<InlineSpan>, Vec<InlineSpan>) {
    let old_chars: Vec<(usize, char)> = old.char_indices().collect();
    let new_chars: Vec<(usize, char)> = new.char_indices().collect();
    let n = old_chars.len();
    let m = new_chars.len();

    // 空行或超长行不做字符级比较，整行高亮反而更清楚。
    const MAX_INLINE_CHARS: usize = 400;
    if n == 0 || m == 0 || n > MAX_INLINE_CHARS || m > MAX_INLINE_CHARS {
        let old_spans = if n == 0 {
            Vec::new()
        } else {
            vec![InlineSpan {
                start: 0,
                end: old.len(),
            }]
        };
        let new_spans = if m == 0 {
            Vec::new()
        } else {
            vec![InlineSpan {
                start: 0,
                end: new.len(),
            }]
        };
        return (old_spans, new_spans);
    }

    // 只保留 LCS 表的长度，回溯所需的走向记在 `directions` 里：
    // 0 = 取对角线（字符相同），1 = 跳过旧行字符，2 = 跳过新行字符。
    // 完整二维 u32 表在 400x400 时是 640KB，这里是一张 160KB 的 u8 表
    // 加两行滚动数组；逻辑不变，分配次数和内存都降一个数量级。
    let mut directions = vec![0u8; (n + 1) * (m + 1)];
    let mut next_row = vec![0u32; m + 1];
    let mut current_row = vec![0u32; m + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let (value, direction) = if old_chars[i].1 == new_chars[j].1 {
                (next_row[j + 1] + 1, 0u8)
            } else if next_row[j] >= current_row[j + 1] {
                (next_row[j], 1u8)
            } else {
                (current_row[j + 1], 2u8)
            };
            current_row[j] = value;
            directions[i * (m + 1) + j] = direction;
        }
        std::mem::swap(&mut next_row, &mut current_row);
    }

    // 沿 LCS 走一遍，命中的字符属于「相同」，其余归为改动段。
    let mut old_spans = Vec::new();
    let mut new_spans = Vec::new();
    let mut old_run: Option<(usize, usize)> = None;
    let mut new_run: Option<(usize, usize)> = None;
    let mut i = 0;
    let mut j = 0;
    while i < n && j < m {
        if old_chars[i].1 == new_chars[j].1 {
            // 相同字符：结束当前改动段。
            push_span(&mut old_spans, old_run.take());
            push_span(&mut new_spans, new_run.take());
            i += 1;
            j += 1;
        } else if directions[i * (m + 1) + j] == 1 {
            let start = old_chars[i].0;
            let end = start + old_chars[i].1.len_utf8();
            old_run = Some(match old_run {
                Some((s, _)) => (s, end),
                None => (start, end),
            });
            i += 1;
        } else {
            let start = new_chars[j].0;
            let end = start + new_chars[j].1.len_utf8();
            new_run = Some(match new_run {
                Some((s, _)) => (s, end),
                None => (start, end),
            });
            j += 1;
        }
    }
    // 尾部剩下的字符。
    if i < n {
        let start = old_chars[i].0;
        old_run = Some(match old_run {
            Some((s, _)) => (s, old.len()),
            None => (start, old.len()),
        });
    }
    if j < m {
        let start = new_chars[j].0;
        new_run = Some(match new_run {
            Some((s, _)) => (s, new.len()),
            None => (start, new.len()),
        });
    }
    push_span(&mut old_spans, old_run);
    push_span(&mut new_spans, new_run);

    (old_spans, new_spans)
}

fn push_span(spans: &mut Vec<InlineSpan>, run: Option<(usize, usize)>) {
    if let Some((start, end)) = run
        && end > start
    {
        spans.push(InlineSpan { start, end });
    }
}

/// 一次 diff 解析后的完整结构。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedDiff {
    /// 按文件分段。每段以 `diff --git` 开头（单文件 diff 也可能没有）。
    pub files: Vec<DiffFile>,
    /// 总新增行数。
    pub added: usize,
    /// 总删除行数。
    pub removed: usize,
}

/// 一个文件的 diff 段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    /// 显示用的路径（优先取 `+++ b/...` 去掉前缀的部分）。
    ///
    /// 删除文件时 `+++ ` 是 `/dev/null`，此时保留 `--- a/...` 里的旧路径。
    pub path: String,
    /// 旧路径。仅重命名/删除时有意义，其余情况等于 `path`。
    pub old_path: Option<String>,
    pub lines: Vec<DiffLine>,
    pub added: usize,
    pub removed: usize,
    /// 二进制文件。这类文件没有可渲染的行，只有一行占位说明。
    pub is_binary: bool,
}

/// 把 `git diff` / `git show` 的 unified 输出解析成结构化行。
///
/// 只依赖行首字符，不解析 hunk 内容本身，所以对任意语言都成立。
pub fn parse_diff(text: &str) -> ParsedDiff {
    let mut files: Vec<DiffFile> = Vec::new();
    let mut current = DiffFile {
        path: String::new(),
        old_path: None,
        lines: Vec::new(),
        added: 0,
        removed: 0,
        is_binary: false,
    };
    // hunk 头里的行号，随着读行递增。
    let mut old_no: u32 = 0;
    let mut new_no: u32 = 0;

    for raw in text.lines() {
        // 新的文件段。
        if raw.starts_with("diff --git ") {
            if !current.lines.is_empty() || !current.path.is_empty() {
                files.push(current);
            }
            // 兜底路径：diff --git a/x b/x 里的后一半。纯重命名没有 `--- ` / `+++ `
            // 行，旧路径只能从这里取。
            let (old_path, fallback) = split_diff_git_paths(raw);
            current = DiffFile {
                path: fallback,
                old_path,
                lines: Vec::new(),
                added: 0,
                removed: 0,
                is_binary: false,
            };
            old_no = 0;
            new_no = 0;
            current.lines.push(DiffLine {
                kind: DiffLineKind::FileHeader,
                old_line: None,
                new_line: None,
                text: raw.to_string(),
            });
            continue;
        }

        if let Some(rest) = raw.strip_prefix("@@ ") {
            if let Some((old_start, new_start)) = parse_hunk_header(rest) {
                old_no = old_start;
                new_no = new_start;
            }
            current.lines.push(DiffLine {
                kind: DiffLineKind::HunkHeader,
                old_line: None,
                new_line: None,
                text: raw.to_string(),
            });
            continue;
        }

        // `+++ b/path` / `--- a/path` 出现在 hunk 之前，属于文件头。
        if raw.starts_with("+++ ") {
            // 删除文件时 `+++ ` 是 `/dev/null`，别拿它覆盖 `--- a/...` 解析出的旧路径。
            let target = raw.trim_start_matches("+++ ");
            if !is_dev_null(target) {
                current.path = strip_diff_prefix(target);
            }
            current.lines.push(DiffLine {
                kind: DiffLineKind::FileHeader,
                old_line: None,
                new_line: None,
                text: raw.to_string(),
            });
            continue;
        }
        // 记下 `--- a/path` 里的旧路径：删除文件时新版路径是 /dev/null，
        // 重命名时两边不同，都需要它。
        if let Some(source) = raw.strip_prefix("--- ")
            && !is_dev_null(source)
        {
            let old_path = strip_diff_prefix(source);
            if current.path.is_empty() {
                current.path = old_path.clone();
            }
            // 已经从 `diff --git` 拿到更准确的两边路径时不要覆盖。
            current.old_path.get_or_insert(old_path);
        }
        if raw.starts_with("--- ")
            || raw.starts_with("index ")
            || raw.starts_with("new file")
            || raw.starts_with("deleted file")
            || raw.starts_with("similarity index")
            || raw.starts_with("rename ")
        {
            current.lines.push(DiffLine {
                kind: DiffLineKind::FileHeader,
                old_line: None,
                new_line: None,
                text: raw.to_string(),
            });
            continue;
        }

        // 二进制文件没有可读内容。git 用一行说明或 `GIT binary patch` + 负载
        // 表达；若不在这里拦下，它们会被当成上下文行，行号还会记成 0。
        // 标记一旦置上，本文件剩下的行都归为同类，直到下一个 `diff --git`。
        if current.is_binary
            || raw.starts_with("Binary files ")
            || raw.starts_with("GIT binary patch")
        {
            current.is_binary = true;
            current.lines.push(DiffLine {
                kind: DiffLineKind::Binary,
                old_line: None,
                new_line: None,
                text: raw.to_string(),
            });
            continue;
        }

        if raw.starts_with('\\') {
            current.lines.push(DiffLine {
                kind: DiffLineKind::Marker,
                old_line: None,
                new_line: None,
                text: raw.to_string(),
            });
            continue;
        }

        let line = if let Some(body) = raw.strip_prefix('+') {
            new_no += 1;
            current.added += 1;
            DiffLine {
                kind: DiffLineKind::Added,
                old_line: None,
                new_line: Some(new_no - 1),
                text: body.to_string(),
            }
        } else if let Some(body) = raw.strip_prefix('-') {
            old_no += 1;
            current.removed += 1;
            DiffLine {
                kind: DiffLineKind::Removed,
                old_line: Some(old_no - 1),
                new_line: None,
                text: body.to_string(),
            }
        } else {
            // 上下文行：可能是 " 文本"，也可能是不带前缀的空行。
            let body = raw.strip_prefix(' ').unwrap_or(raw);
            let line = DiffLine {
                kind: DiffLineKind::Context,
                old_line: Some(old_no),
                new_line: Some(new_no),
                text: body.to_string(),
            };
            old_no += 1;
            new_no += 1;
            line
        };

        current.lines.push(line);
    }

    if !current.lines.is_empty() || !current.path.is_empty() {
        files.push(current);
    }

    let added = files.iter().map(|file| file.added).sum();
    let removed = files.iter().map(|file| file.removed).sum();
    ParsedDiff {
        files,
        added,
        removed,
    }
}

/// 从 diff 里还原某个文件的新版内容，供语法高亮使用。
///
/// 直接用 hunk 行拼装：上下文行和新增行就是新文件里连续的内容。行号不连续
/// 时（跳过的未改动区块）用空行补齐，保证数组下标 == 行号 - 1，高亮时能按
/// 行号直接取到对应的源码。
pub fn reconstructed_new_lines(file: &DiffFile) -> Vec<String> {
    if file.is_binary {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::new();
    for line in &file.lines {
        match line.kind {
            DiffLineKind::Added | DiffLineKind::Context => {
                if let Some(new_line) = line.new_line {
                    let index = (new_line as usize).saturating_sub(1);
                    if lines.len() < index {
                        lines.resize(index, String::new());
                    }
                    if lines.len() == index {
                        lines.push(line.text.clone());
                    }
                }
            }
            _ => {}
        }
    }
    lines
}

/// 从 diff 里还原某个文件的旧版内容，供删除行的语法高亮使用。
///
/// 和 [`reconstructed_new_lines`] 对称：上下文行和删除行拼出旧文件。删行在
/// 单栏和并排视图里都占一席之地，给它配上语法高亮，观感才和新增行一致。
pub fn reconstructed_old_lines(file: &DiffFile) -> Vec<String> {
    if file.is_binary {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::new();
    for line in &file.lines {
        match line.kind {
            DiffLineKind::Removed | DiffLineKind::Context => {
                if let Some(old_line) = line.old_line {
                    let index = (old_line as usize).saturating_sub(1);
                    if lines.len() < index {
                        lines.resize(index, String::new());
                    }
                    if lines.len() == index {
                        lines.push(line.text.clone());
                    }
                }
            }
            _ => {}
        }
    }
    lines
}

/// 从 hunk 头 `-a,b +c,d @@ ...` 里取新旧起始行号。
fn parse_hunk_header(rest: &str) -> Option<(u32, u32)> {
    let mut parts = rest.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let old_start = old.split(',').next()?.parse().ok()?;
    let new_start = new.split(',').next()?.parse().ok()?;
    Some((old_start, new_start))
}

/// 从 `diff --git a/x b/y` 里拆出新旧路径。
///
/// 路径里可能含空格，所以按最后一个 ` b/` 切；只有新旧不同时才返回旧路径。
fn split_diff_git_paths(raw: &str) -> (Option<String>, String) {
    let rest = raw.trim_start_matches("diff --git ");
    let Some((old_part, new_part)) = rest.rsplit_once(" b/") else {
        return (None, rest.to_string());
    };
    let old_path = strip_diff_prefix(old_part);
    let new_path = new_part.to_string();
    if old_path == new_path {
        (None, new_path)
    } else {
        (Some(old_path), new_path)
    }
}

/// `/dev/null` 在 diff 里表示「这一侧不存在」。
fn is_dev_null(path: &str) -> bool {
    path.trim() == "/dev/null"
}

/// 去掉 `a/` `b/` 前缀。
fn strip_diff_prefix(path: &str) -> String {
    let path = path.trim();
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
        .to_string()
}

/// git 子进程的最长等待时间。
///
/// 没有超时的话，仓库在网络盘上、文件系统卡住、或 git 等索引锁
/// （`.git/index.lock`）时会永久阻塞，调用方的后台线程再也回不来。
const GIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// 写操作（commit）的超时。跑 hook 可能比只读命令慢得多。
const GIT_COMMIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// 跑一个 git 命令，带超时，返回 (stdout, stderr, 是否成功)。
fn run_git_raw(
    repo: &Path,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<(String, String, bool), String> {
    // stdout/stderr 用管道，由后台线程读取；主线程只负责等待 + 超时。
    // 直接 `output()` 只能同步等待，没法半路放弃。
    let mut child = Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法运行 git: {error}"))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let read_all = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buffer);
            }
            buffer
        })
    };
    let stdout_reader = read_all(stdout.map(|s| Box::new(s) as Box<_>));
    let stderr_reader = read_all(stderr.map(|s| Box::new(s) as Box<_>));

    // 轮询子进程状态：标准库没有 wait_timeout，用短睡眠即可。
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "git {} 超时（{} 秒未返回）",
                        args.first().copied().unwrap_or(""),
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(error) => return Err(format!("等待 git 失败: {error}")),
        }
    };

    let out = stdout_reader.join().unwrap_or_default();
    let err = stderr_reader.join().unwrap_or_default();
    let out = String::from_utf8(out).map_err(|_| "git 输出不是有效 UTF-8".to_string())?;
    let err = String::from_utf8_lossy(&err).into_owned();
    Ok((out, err, status.success()))
}

fn run_git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let (stdout, stderr, success) = run_git_raw(repo, args, GIT_TIMEOUT)?;
    if !success {
        return Err(stderr.trim().to_string());
    }
    Ok(stdout)
}

/// 仓库根。`Ok(None)` 表示不是 git 仓库，`Err` 表示 git 本身出了问题
/// （未安装、超时等），两者在界面上应该区分开。
pub fn repository_root(start: &Path) -> Result<Option<PathBuf>, String> {
    let (stdout, _stderr, success) =
        run_git_raw(start, &["rev-parse", "--show-toplevel"], GIT_TIMEOUT)?;
    if !success {
        // git 能跑但目录不是仓库：正常的空状态。
        return Ok(None);
    }
    let trimmed = stdout.trim();
    Ok((!trimmed.is_empty()).then(|| PathBuf::from(trimmed)))
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

        // 冲突状态必须按两列的组合识别。单看某个字符会把 `UU` 读成
        // 「暂存区未改、工作区未合并」，用户看不出这是需要手工解决的冲突。
        let is_conflicted = matches!(
            (x, y),
            ('D', 'D') | ('A', 'U') | ('U', 'D') | ('U', 'A') | ('D', 'U') | ('A', 'A')
        ) || (x == 'U' && y == 'U');

        // `??` 表示未跟踪：两列都是 `?`，但它只在工作区里，暂存区没有这项。
        // 直接按字符解析会让同一个文件同时出现在「已暂存」和「未暂存」里，
        // 计数也会翻倍。
        let is_untracked = x == '?' || y == '?';

        let (staged, unstaged) = if is_conflicted {
            (None, Some(GitStatus::Unmerged))
        } else if is_untracked {
            (None, Some(GitStatus::Untracked))
        } else {
            (GitStatus::from_char(x), GitStatus::from_char(y))
        };

        // 重命名/复制在 -z 模式下紧接着一个原路径字段，顺序是「新路径 \0 旧路径」。
        // 丢掉它会导致重命名的 diff 只能看到新路径，git 会把整篇当成新增。
        let old_path = if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') {
            fields.next().map(PathBuf::from)
        } else {
            None
        };
        let old_path = old_path.filter(|old| old.as_os_str() != path);

        changes.push(GitChange {
            path: PathBuf::from(path),
            old_path,
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
        let (path, old_path) = if matches!(status_char, 'R' | 'C') {
            let old_path = first_path;
            let new_path = fields.next().unwrap_or(old_path);
            (new_path, Some(old_path))
        } else {
            (first_path, None)
        };
        let old_path = old_path
            .map(PathBuf::from)
            .filter(|old| old.as_os_str() != path);
        changes.push(GitChange {
            path: PathBuf::from(path),
            old_path,
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

/// 工作区里某个文件相对 HEAD 的改动。
///
/// 用 `diff HEAD` 而不是裸 `diff`：后者只显示未暂存的改动，暂存之后
/// diff 视图会突然变空，看起来像文件没变化。
///
/// 两种情况要特殊处理：
/// - 未跟踪文件不在索引里，`diff HEAD` 对它没有任何输出，改用
///   `--no-index /dev/null <path>` 才能看到内容；
/// - 重命名需要 `-M` 且**两边路径都要传**，否则 git 只看新路径，
///   会把整个文件当成新增。
pub fn working_diff(repo: &Path, change: &GitChange) -> Result<GitDiff, String> {
    let path = &change.path;
    let is_untracked = change.unstaged == Some(GitStatus::Untracked) && change.staged.is_none();
    if is_untracked {
        return untracked_diff(repo, path);
    }

    let path_text = path.to_string_lossy().into_owned();
    let old_text = change
        .old_path
        .as_ref()
        .filter(|old| *old != path)
        .map(|old| old.to_string_lossy().into_owned());

    let mut args = vec!["diff", "HEAD", "-M", "--no-color", "--no-ext-diff", "--"];
    args.push(&path_text);
    if let Some(old) = old_text.as_deref() {
        args.push(old);
    }
    let text = run_git(repo, &args)?;
    Ok(GitDiff {
        text: text.trim_start_matches('\n').to_string(),
    })
}

/// 未跟踪文件的完整内容，伪装成「从 /dev/null 新增」的 diff。
///
/// `git diff --no-index` 在「有差异」时退出码是 1，那不是错误。
fn untracked_diff(repo: &Path, path: &Path) -> Result<GitDiff, String> {
    let path_text = path.to_string_lossy().into_owned();
    // 不能直接用 run_git：`--no-index` 在「有差异」时退出码是 1，
    // 那是正常结果而不是错误，需要拿到原始 stdout 才判断得了。
    let (stdout, stderr, success) = run_git_raw(
        repo,
        &[
            "diff",
            "--no-index",
            "--no-color",
            "--no-ext-diff",
            "--",
            "/dev/null",
            &path_text,
        ],
        GIT_TIMEOUT,
    )?;
    if !success && !stderr.trim().is_empty() {
        return Err(stderr.trim().to_string());
    }
    Ok(GitDiff {
        text: stdout.trim_start_matches('\n').to_string(),
    })
}

/// 整个工作区相对 HEAD 的改动，供自动刷新重新读取。
pub fn working_diff_all(repo: &Path) -> Result<String, String> {
    let text = run_git(repo, &["diff", "HEAD", "-M", "--no-color", "--no-ext-diff"])?;
    Ok(text.trim_start_matches('\n').to_string())
}

/// 提交暂存区。`message` 为空时返回错误，不执行任何操作。
/// 提交。`stage_all` 为 true 时先把已跟踪文件的改动全部暂存。
///
/// 对应 VSCode 的「smart commit」：暂存区为空时，直接提交工作区里所有
/// 已跟踪文件的改动（等价于 `git commit -a`）。未跟踪的新文件不会被
/// 带上——那需要显式 `git add`，和 git 本身的行为一致。
pub fn commit(repo: &Path, message: &str, stage_all: bool) -> Result<String, String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("提交信息不能为空".to_string());
    }
    let mut args = vec!["commit"];
    if stage_all {
        args.push("-a");
    }
    args.push("-m");
    args.push(message);

    // `git commit` 在「没有可提交内容」时**退出码仍是 0**，只把说明打到
    // stdout/stderr。按退出码判断会让界面显示提交成功，实际什么都没发生。
    // 提交要写索引、跑 hook，比只读命令慢，给更长的超时；
    // 但仍然不能没有上限，否则 hook 卡住会拖死调用方线程。
    let (stdout, stderr, success) = run_git_raw(repo, &args, GIT_COMMIT_TIMEOUT)?;

    // 「无内容可提交」要单独识别：git 的说明打在 stdout，退出码视情况为 0 或 1，
    // 直接透传会得到空错误文本。
    if !committed_something(&stdout, &stderr) {
        return Err("没有可提交的改动。未跟踪文件需要先暂存。".to_string());
    }
    if !success {
        let message = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        return Err(message.to_string());
    }
    Ok(stdout.trim().to_string())
}

/// 判断这次 `git commit` 是否真的产生了提交。
///
/// git 的空提交提示可能出现在 stdout 或 stderr，措辞有几种变体。
fn committed_something(stdout: &str, stderr: &str) -> bool {
    const EMPTY_MARKERS: [&str; 4] = [
        "nothing to commit",
        "nothing added to commit",
        "no changes added to commit",
        "nothing staged to commit",
    ];
    let combined = format!("{stdout}\n{stderr}");
    !EMPTY_MARKERS.iter().any(|marker| combined.contains(marker))
}

/// 把某个文件加入暂存区（`git add`）。
///
/// 未跟踪文件和已修改文件都适用：`git add` 会顺带开始跟踪新文件。
pub fn stage_file(repo: &Path, path: &Path) -> Result<(), String> {
    let path_text = path.to_string_lossy().into_owned();
    run_git(repo, &["add", "--", &path_text])?;
    Ok(())
}

/// 取消暂存（`git restore --staged`），工作区内容保持不变。
pub fn unstage_file(repo: &Path, path: &Path) -> Result<(), String> {
    let path_text = path.to_string_lossy().into_owned();
    run_git(repo, &["restore", "--staged", "--", &path_text])?;
    Ok(())
}

/// 放弃某个文件的改动，**丢弃内容不可恢复**。
///
/// `untracked` 决定用哪条命令：
/// - 已跟踪文件用 `git restore`，还原到 HEAD 的内容；
/// - 未跟踪文件 `git restore` 会报 "did not match any file(s)"，只能用
///   `git clean -f` 删除。
///
/// 注意这里不加 `--staged`：同时还原工作区和暂存区，符合「放弃这处改动」
/// 的直觉。只想撤销暂存的话应该用 `git restore --staged`，那是另一个动作。
///
/// `change` 是调用方手里的那条变更记录。冲突文件上 `git restore` 会报
/// "path is unmerged" 失败，只能改用 `checkout --ours`，所以需要状态信息；
/// 由调用方传入而不是自己再跑一次 `git status`——后者要扫整个仓库。
pub fn discard_change(repo: &Path, change: &GitChange) -> Result<(), String> {
    let path = &change.path;
    let path_text = path.to_string_lossy().into_owned();
    if change.unstaged == Some(GitStatus::Untracked) {
        run_git(repo, &["clean", "-f", "--", &path_text])?;
    } else if change.unstaged == Some(GitStatus::Unmerged) {
        // 取本方内容，这是「放弃我的修改」最接近的语义，且不留冲突标记。
        run_git(repo, &["checkout", "--ours", "--", &path_text])?;
        // `checkout --ours` 只改工作区，文件仍在冲突状态；
        // 显式 add 让冲突解决完成，否则列表里还是红色 U。
        run_git(repo, &["add", "--", &path_text])?;
    } else {
        run_git(repo, &["restore", "--", &path_text])?;
    }
    Ok(())
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

        // "??" -> 未跟踪，只出现在工作区，暂存区没有这项。
        assert_eq!(changes[1].path, PathBuf::from("new_file.txt"));
        assert_eq!(changes[1].staged, None, "未跟踪文件不该算已暂存");
        assert_eq!(changes[1].unstaged, Some(GitStatus::Untracked));
        assert!(!changes[1].is_staged());

        // "A " -> 已暂存新增。
        assert_eq!(changes[2].staged, Some(GitStatus::Added));
        assert_eq!(changes[2].unstaged, None);
        assert!(changes[2].is_staged());
    }

    /// 未跟踪文件只能被一个分组统计，否则「已暂存 1 / 未暂存 6」会明显对不上。
    #[test]
    fn untracked_file_is_counted_once() {
        let text = " M a.rs\0?? todo.yml\0";
        let changes = parse_status(text);
        let staged = changes.iter().filter(|c| c.is_staged()).count();
        let unstaged = changes.iter().filter(|c| c.is_unstaged()).count();
        assert_eq!(staged, 0, "没有任何暂存内容");
        assert_eq!(unstaged, 2);
        assert_eq!(staged + unstaged, changes.len(), "每个文件只归一组");
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

    /// 暂存区为空时 smart commit：提交已跟踪文件的改动，未跟踪文件不动。
    #[test]
    fn smart_commit_stages_tracked_files_only() {
        let dir = std::env::temp_dir().join(format!(
            "con-git-smart-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git 应当可用")
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "T"]);
        std::fs::write(dir.join("tracked.txt"), "base\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "init"]);

        std::fs::write(dir.join("tracked.txt"), "base\nmore\n").unwrap();
        std::fs::write(dir.join("untracked.txt"), "new\n").unwrap();

        commit(&dir, "smart", true).unwrap();

        // 已跟踪的改动被提交，未跟踪文件仍然留在工作区。
        assert!(dir.join("untracked.txt").exists(), "未跟踪文件不该被提交");
        let status = status(&dir).unwrap();
        assert_eq!(status.len(), 1, "只剩未跟踪文件");
        assert_eq!(status[0].unstaged, Some(GitStatus::Untracked));

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 已跟踪文件：restore 回 HEAD；未跟踪文件：直接删掉。
    ///
    /// 用真实临时仓库跑，因为这两条命令的差异（restore 对未跟踪文件会报错）
    /// 没法用 mock 表达。
    #[test]
    fn discard_handles_tracked_and_untracked_differently() {
        let dir = std::env::temp_dir().join(format!(
            "con-git-discard-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let init = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git 应当可用")
        };
        init(&["init", "-q"]);
        init(&["config", "user.email", "t@t"]);
        init(&["config", "user.name", "T"]);

        std::fs::write(dir.join("tracked.txt"), "base\n").unwrap();
        init(&["add", "."]);
        init(&["commit", "-qm", "init"]);

        // 制造一处修改和一个未跟踪文件。
        std::fs::write(dir.join("tracked.txt"), "base\nmodified\n").unwrap();
        std::fs::write(dir.join("untracked.txt"), "new\n").unwrap();

        discard_change(
            &dir,
            &GitChange {
                path: PathBuf::from("tracked.txt"),
                old_path: None,
                staged: None,
                unstaged: Some(GitStatus::Modified),
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("tracked.txt")).unwrap(),
            "base\n",
            "已跟踪文件应还原到 HEAD 内容"
        );

        discard_change(
            &dir,
            &GitChange {
                path: PathBuf::from("untracked.txt"),
                old_path: None,
                staged: None,
                unstaged: Some(GitStatus::Untracked),
            },
        )
        .unwrap();
        assert!(!dir.join("untracked.txt").exists(), "未跟踪文件应被删除");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_commit_message_is_rejected_without_running_git() {
        // 指向不存在的目录也能通过：空信息在调用 git 之前就被拦下了。
        let error = commit(Path::new("/nonexistent/con-git-test"), "   ", false).unwrap_err();
        assert!(error.contains("不能为空"));
    }

    #[test]
    fn parses_diff_line_kinds_and_line_numbers() {
        let text = "\
diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -10,3 +10,4 @@ fn main() {
 let a = 1;
-let b = 2;
+let b = 3;
+let c = 4;
 let d = 5;
";
        let diff = parse_diff(text);

        assert_eq!(diff.files.len(), 1);
        let file = &diff.files[0];
        assert_eq!(file.path, "src/a.rs");
        assert_eq!(file.added, 2);
        assert_eq!(file.removed, 1);
        assert_eq!(diff.added, 2);
        assert_eq!(diff.removed, 1);

        // hunk 头从 10/10 开始，上下文行占旧新两个行号。
        let context: Vec<_> = file
            .lines
            .iter()
            .filter(|line| line.kind == DiffLineKind::Context)
            .collect();
        assert_eq!(context[0].old_line, Some(10));
        assert_eq!(context[0].new_line, Some(10));
        assert_eq!(context[0].text, "let a = 1;");

        let removed: Vec<_> = file
            .lines
            .iter()
            .filter(|line| line.kind == DiffLineKind::Removed)
            .collect();
        assert_eq!(removed[0].old_line, Some(11));
        assert_eq!(removed[0].new_line, None, "删除行没有新行号");

        let added: Vec<_> = file
            .lines
            .iter()
            .filter(|line| line.kind == DiffLineKind::Added)
            .collect();
        assert_eq!(added[0].new_line, Some(11));
        assert_eq!(added[0].old_line, None, "新增行没有旧行号");
        assert_eq!(added[1].new_line, Some(12));
    }

    #[test]
    fn parses_multiple_files_in_one_diff() {
        let text = "\
diff --git a/one.txt b/one.txt
--- a/one.txt
+++ b/one.txt
@@ -1 +1 @@
-old
+new
diff --git a/two.txt b/two.txt
--- a/two.txt
+++ b/two.txt
@@ -5 +5,2 @@
 keep
+extra
";
        let diff = parse_diff(text);

        assert_eq!(diff.files.len(), 2);
        assert_eq!(diff.files[0].path, "one.txt");
        assert_eq!(diff.files[1].path, "two.txt");
        assert_eq!(diff.added, 2);
        assert_eq!(diff.removed, 1);
    }

    /// hunk 头里的行号必须重置每一段，否则第二段的编号会接着第一段往下跑。
    #[test]
    fn line_numbers_reset_per_hunk() {
        let text = "\
diff --git a/x b/x
--- a/x
+++ b/x
@@ -1,2 +1,2 @@
 a
-b
+c
@@ -100,1 +100,1 @@
-d
+e
";
        let diff = parse_diff(text);
        let added: Vec<_> = diff.files[0]
            .lines
            .iter()
            .filter(|line| line.kind == DiffLineKind::Added)
            .collect();

        assert_eq!(added[0].new_line, Some(2), "第一个 hunk 的新行号");
        assert_eq!(
            added[1].new_line,
            Some(100),
            "第二个 hunk 从 @@ +100 重新开始"
        );
    }

    #[test]
    fn empty_diff_yields_no_files() {
        let diff = parse_diff("");
        assert!(diff.files.is_empty());
        assert_eq!(diff.added, 0);
        assert_eq!(diff.removed, 0);
    }

    /// `\\ No newline at end of file` 不该被算成增删行。
    #[test]
    fn marker_lines_are_not_counted_as_changes() {
        let text = "\
diff --git a/x b/x
--- a/x
+++ b/x
@@ -1 +1 @@
-a
\\ No newline at end of file
+b
";
        let diff = parse_diff(text);
        assert_eq!(diff.added, 1);
        assert_eq!(diff.removed, 1);
        assert!(
            diff.files[0]
                .lines
                .iter()
                .any(|line| line.kind == DiffLineKind::Marker)
        );
    }

    /// 重建的新文件内容必须与行号对齐，且包含新增行。
    #[test]
    fn reconstructs_new_file_content_aligned_to_line_numbers() {
        let text = "\
diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1,3 +1,4 @@
 fn a() {}
-old
+new
+extra
 tail
";
        let diff = parse_diff(text);
        let lines = reconstructed_new_lines(&diff.files[0]);

        assert_eq!(lines[0], "fn a() {}");
        assert_eq!(lines[1], "new");
        assert_eq!(lines[2], "extra");
        assert_eq!(lines[3], "tail");
    }

    /// 跳过未改动区块时用空行补齐，保证下标与行号一致。
    #[test]
    fn reconstruction_pads_skipped_ranges() {
        let text = "\
diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -10,1 +10,1 @@
-ten
+new_ten
";
        let diff = parse_diff(text);
        let lines = reconstructed_new_lines(&diff.files[0]);

        // 第 10 行是 new_ten，也就是下标 9。
        assert_eq!(lines.len(), 10);
        assert_eq!(lines[9], "new_ten");
    }

    /// 二进制文件的说明行不能被当成上下文行。
    #[test]
    fn binary_file_is_marked_and_not_numbered() {
        let text = "\
diff --git a/logo.png b/logo.png
index 111..222 100644
Binary files a/logo.png and b/logo.png differ
";
        let diff = parse_diff(text);
        let file = &diff.files[0];

        assert!(file.is_binary);
        assert_eq!(file.path, "logo.png");
        assert_eq!(file.added, 0);
        assert_eq!(file.removed, 0);
        assert!(
            file.lines
                .iter()
                .all(|line| line.kind != DiffLineKind::Context),
            "二进制说明行不该被当成上下文"
        );
        assert!(
            file.lines
                .iter()
                .any(|line| line.kind == DiffLineKind::Binary),
            "应当有 Binary 行"
        );
        // 二进制文件不参与高亮重建。
        assert!(reconstructed_new_lines(file).is_empty());
        assert!(reconstructed_old_lines(file).is_empty());
    }

    /// `GIT binary patch` 及其负载也归为二进制，且不污染后续文件。
    #[test]
    fn git_binary_patch_body_stays_binary_and_resets_on_next_file() {
        let text = "\
diff --git a/img.png b/img.png
index 111..222 100644
GIT binary patch
literal 12
TcmZ?wbhEHbRA>iY0RR91

diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1,1 +1,1 @@
-old
+new
";
        let diff = parse_diff(text);
        assert!(diff.files[0].is_binary);
        assert!(
            diff.files[0]
                .lines
                .iter()
                .all(|line| line.kind != DiffLineKind::Context),
            "补丁负载不能被当成上下文"
        );

        let second = &diff.files[1];
        assert!(!second.is_binary, "标记必须随文件段重置");
        assert_eq!(second.added, 1);
        assert_eq!(second.removed, 1);
    }

    /// 删除文件的路径应保留旧路径，而不是变成 /dev/null。
    #[test]
    fn deleted_file_keeps_its_old_path() {
        let text = "\
diff --git a/old.rs b/old.rs
deleted file mode 100644
index 333..000
--- a/old.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-fn gone() {}
-more
";
        let diff = parse_diff(text);
        let file = &diff.files[0];

        assert_eq!(file.path, "old.rs");
        assert_eq!(file.old_path.as_deref(), Some("old.rs"));
        assert_eq!(file.removed, 2);
        assert_eq!(file.added, 0);
    }

    /// 新增文件：`--- /dev/null`，路径取 `+++ b/...`。
    #[test]
    fn added_file_uses_the_new_path() {
        let text = "\
diff --git a/new.rs b/new.rs
new file mode 100644
index 000..444
--- /dev/null
+++ b/new.rs
@@ -0,0 +1,2 @@
+one
+two
";
        let diff = parse_diff(text);
        let file = &diff.files[0];

        assert_eq!(file.path, "new.rs");
        assert_eq!(file.old_path, None, "/dev/null 不是有效旧路径");
        assert_eq!(file.added, 2);
    }

    /// 重命名记录两边路径。
    #[test]
    fn renamed_file_records_both_paths() {
        let text = "\
diff --git a/old_name.rs b/new_name.rs
similarity index 100%
rename from old_name.rs
rename to new_name.rs
";
        let diff = parse_diff(text);
        let file = &diff.files[0];

        assert_eq!(file.path, "new_name.rs");
        assert_eq!(file.old_path.as_deref(), Some("old_name.rs"));
    }

    /// 只有未跟踪文件时 `git commit -a` 退出码是 0 但没提交任何东西，
    /// 必须在 API 层识别成错误，否则界面会假装成功。
    #[test]
    fn commit_with_only_untracked_files_reports_an_error() {
        let dir = std::env::temp_dir().join(format!(
            "con-git-empty-commit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git 应当可用")
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "T"]);
        std::fs::write(dir.join("base.txt"), "base\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "init"]);

        // 只有一个未跟踪文件：`-a` 不会带上它。
        std::fs::write(dir.join("new.txt"), "new\n").unwrap();
        let error = commit(&dir, "尝试提交", true).unwrap_err();
        assert!(
            error.contains("没有可提交的改动"),
            "应给出明确提示，实际: {error}"
        );

        // 未跟踪文件确实还在工作区，没有被吞掉。
        assert!(dir.join("new.txt").exists());

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 有真实改动时正常提交，不能误报。
    #[test]
    fn commit_with_real_changes_succeeds() {
        let dir = std::env::temp_dir().join(format!(
            "con-git-real-commit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git 应当可用")
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "T"]);
        std::fs::write(dir.join("base.txt"), "base\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "init"]);

        std::fs::write(dir.join("base.txt"), "base\nmore\n").unwrap();
        let output = commit(&dir, "真实提交", true).unwrap();
        assert!(output.contains("真实提交"), "输出里应包含提交信息");

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 超时必须返回错误而不是永久挂起。
    #[test]
    fn run_git_times_out_instead_of_hanging() {
        let started = std::time::Instant::now();
        let result = run_git_raw(
            Path::new("/tmp"),
            &["-c", "alias.hang=!sleep 5", "hang"],
            std::time::Duration::from_millis(200),
        );
        assert!(result.is_err(), "超时应当返回错误");
        assert!(result.unwrap_err().contains("超时"));
        // 200ms 超时 + 轮询粒度，远小于它要 sleep 的 5 秒。
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "不应该等到子进程自然结束"
        );
    }

    /// 冲突状态按两列组合识别，不能拆成单个字符。
    #[test]
    fn merge_conflicts_are_parsed_as_unmerged() {
        let text = "UU f.txt\0AA g.txt\0DU h.txt\0";
        let changes = parse_status(text);
        assert_eq!(changes.len(), 3);
        for change in &changes {
            assert_eq!(
                change.unstaged,
                Some(GitStatus::Unmerged),
                "{} 应识别为冲突",
                change.path.display()
            );
            assert!(
                !change.is_staged(),
                "{} 冲突时不属于暂存区",
                change.path.display()
            );
        }
    }

    /// 冲突文件上的「放弃更改」不能直接报错，且要真正解决冲突。
    #[test]
    fn discard_resolves_conflict_with_ours() {
        let dir = std::env::temp_dir().join(format!(
            "con-git-conflict-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git 应当可用")
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "T"]);
        std::fs::write(dir.join("f.txt"), "a\nb\nc\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "base"]);
        git(&["checkout", "-qb", "other"]);
        std::fs::write(dir.join("f.txt"), "a\nOTHER\nc\n").unwrap();
        git(&["commit", "-qam", "other"]);
        git(&["checkout", "-q", "-"]);
        std::fs::write(dir.join("f.txt"), "a\nMINE\nc\n").unwrap();
        git(&["commit", "-qam", "mine"]);
        git(&["merge", "other"]);

        let before = status(&dir).unwrap();
        assert!(
            before
                .iter()
                .any(|c| c.unstaged == Some(GitStatus::Unmerged)),
            "前置条件：应当处于冲突状态"
        );

        discard_change(
            &dir,
            &GitChange {
                path: PathBuf::from("f.txt"),
                old_path: None,
                staged: None,
                unstaged: Some(GitStatus::Unmerged),
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.join("f.txt")).unwrap();
        assert_eq!(content, "a\nMINE\nc\n", "应保留本方内容");
        assert!(!content.contains("<<<<<<<"), "不能留下冲突标记");

        let after = status(&dir).unwrap();
        assert!(
            after
                .iter()
                .all(|c| c.unstaged != Some(GitStatus::Unmerged)),
            "放弃后不应再是冲突状态"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 暂存与取消暂存要能来回切换，且不影响工作区内容。
    #[test]
    fn stage_and_unstage_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "con-git-stage-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git 应当可用")
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "T"]);
        std::fs::write(dir.join("tracked.txt"), "one\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "init"]);

        std::fs::write(dir.join("tracked.txt"), "two\n").unwrap();
        stage_file(&dir, Path::new("tracked.txt")).unwrap();
        let staged = status(&dir).unwrap();
        assert!(
            staged
                .iter()
                .find(|c| c.path == Path::new("tracked.txt"))
                .unwrap()
                .is_staged(),
            "暂存后应出现在暂存区"
        );

        unstage_file(&dir, Path::new("tracked.txt")).unwrap();
        let after = status(&dir).unwrap();
        let entry = after
            .iter()
            .find(|c| c.path == Path::new("tracked.txt"))
            .unwrap();
        assert!(!entry.is_staged(), "取消暂存后不该在暂存区");
        assert_eq!(
            std::fs::read_to_string(dir.join("tracked.txt")).unwrap(),
            "two\n",
            "取消暂存不能改动工作区内容"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 未跟踪文件暂存后进入暂存区，不再算作未跟踪。
    #[test]
    fn staging_untracked_file_begins_tracking() {
        let dir = std::env::temp_dir().join(format!(
            "con-git-stage-untracked-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git 应当可用")
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "T"]);
        std::fs::write(dir.join(".gitkeep"), "").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "init"]);

        std::fs::write(dir.join("new.txt"), "hello\n").unwrap();
        let before = status(&dir).unwrap();
        let entry = before
            .iter()
            .find(|c| c.path == Path::new("new.txt"))
            .unwrap();
        assert_eq!(entry.unstaged, Some(GitStatus::Untracked));

        stage_file(&dir, Path::new("new.txt")).unwrap();
        let after = status(&dir).unwrap();
        let entry = after
            .iter()
            .find(|c| c.path == Path::new("new.txt"))
            .unwrap();
        assert_eq!(entry.staged, Some(GitStatus::Added));
        assert_eq!(entry.unstaged, None);

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 旧版内容还原：删除行按旧行号落位，新增行不参与。
    #[test]
    fn reconstructs_old_file_content_for_removed_lines() {
        let text = "\
diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1,3 +1,3 @@
 fn a() {}
-old
+new
 tail
";
        let diff = parse_diff(text);
        let lines = reconstructed_old_lines(&diff.files[0]);

        assert_eq!(lines, vec!["fn a() {}", "old", "tail"]);
    }

    #[test]
    fn inline_diff_finds_the_changed_fragment() {
        let (old_spans, new_spans) = inline_diff("let a = 1;", "let a = 2;");

        // 只有最后那个字符不同。
        assert_eq!(old_spans, vec![InlineSpan { start: 8, end: 9 }]);
        assert_eq!(new_spans, vec![InlineSpan { start: 8, end: 9 }]);
    }

    #[test]
    fn inline_diff_marks_insertions_as_empty_on_the_old_side() {
        let (old_spans, new_spans) = inline_diff("abc", "abcdef");
        assert!(old_spans.is_empty(), "旧行没有改动");
        assert_eq!(new_spans, vec![InlineSpan { start: 3, end: 6 }]);
    }

    #[test]
    fn inline_diff_handles_multibyte_characters() {
        let (old_spans, new_spans) = inline_diff("名字", "名号");
        // 第一个汉字 3 字节，改动从偏移 3 开始。
        assert_eq!(old_spans, vec![InlineSpan { start: 3, end: 6 }]);
        assert_eq!(new_spans, vec![InlineSpan { start: 3, end: 6 }]);
    }

    #[test]
    fn inline_diff_of_identical_lines_has_no_spans() {
        let (old_spans, new_spans) = inline_diff("same", "same");
        assert!(old_spans.is_empty());
        assert!(new_spans.is_empty());
    }

    /// 超长行不做字符级比较，整行算改动。
    #[test]
    fn inline_diff_falls_back_to_whole_line_when_too_long() {
        let long_old = "a".repeat(500);
        let long_new = "b".repeat(500);
        let (old_spans, new_spans) = inline_diff(&long_old, &long_new);
        assert_eq!(old_spans.len(), 1);
        assert_eq!(old_spans[0].end, long_old.len());
        assert_eq!(new_spans.len(), 1);
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
