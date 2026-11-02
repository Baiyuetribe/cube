//! 用户维护的远程服务器列表。
//!
//! 存在 `$CONFIG/connections.json`，与 `con.conf` 分开：这些是结构化记录，
//! 用行式配置语法表达会退化成带序号的键，增删时容易错位。
//!
//! 认证走密钥或密码。密码按用户选择**明文存盘**，因此这个文件要当作凭据
//! 对待：它不进版本库，也不该被同步到公开位置。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 一条保存的连接。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SshConnection {
    /// 稳定标识，用于 UI 的元素身份。
    ///
    /// 列表按分组重排、删除后下标前移，所以不能拿下标当元素 id：那样 GPUI
    /// 会把上一帧的行状态套到新的一行上，表现为删除后行内容与悬停位置错位。
    /// 旧文件没有这个字段，`normalize` 会补一个。
    pub id: String,
    /// 显示名，也是列表里的唯一标识。允许重复，但界面只会点中第一个。
    pub name: String,
    pub host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// 私钥路径。为空时由 ssh 自行选择（通常是 `~/.ssh/id_*`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    /// 登录密码。为空表示走密钥或由 ssh 当场提示。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// 分组名，仅用于列表归类，可为空。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

impl Default for SshConnection {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            host: String::new(),
            port: None,
            user: None,
            identity_file: None,
            password: None,
            group: None,
        }
    }
}

impl SshConnection {
    /// 交给终端执行的 `ssh` 参数。
    ///
    /// 以 `-o StrictHostKeyChecking=accept-new` 让首次连接不再卡在 yes/no 提示
    /// 上，同时仍然拒绝已变更的主机密钥。
    pub fn ssh_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
        ];
        if let Some(port) = self.port
            && port != 22
        {
            args.push("-p".to_string());
            args.push(port.to_string());
        }
        if let Some(key) = self.identity_file.as_deref().map(str::trim)
            && !key.is_empty()
        {
            args.push("-i".to_string());
            args.push(expand_home(key));
        }
        let target = match self
            .user
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
        {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        };
        args.push(target);
        args
    }

    /// 交给 shell 执行的单行命令。
    ///
    /// 连接是用户在浮层里敲出来的字符串，直接拼进 shell 会把 `;`、空格、
    /// `$` 当成语法。逐项加单引号，让整行按字面值传给 ssh。
    ///
    /// 用 `command ssh` 而不是裸 `ssh`：用户自己的 rc 文件可能把 `ssh` 定义成
    /// 函数（Ghostty 的 shell 集成就会这么做），而那一层包装未必指向可用的
    /// 二进制。连接功能要的就是系统 ssh。
    pub fn shell_command(&self) -> String {
        std::iter::once("command ssh".to_string())
            .chain(self.ssh_args().into_iter().map(|arg| shell_quote(&arg)))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// 带密码的 shell 命令。
    ///
    /// 密码**不**出现在命令行里——否则它会留在终端的回滚缓冲、shell 历史和
    /// 进程列表里。这里通过 `SSH_ASKPASS` 交给 ssh：脚本由
    /// [`write_askpass_script`] 写到临时目录，命令结束后连同目录一起删除。
    pub fn shell_command_with_askpass(&self, askpass: &std::path::Path) -> String {
        let script = shell_quote(&askpass.to_string_lossy());
        // 脚本所在的一次性目录也要一起收掉，否则每连一次就留一个空目录。
        let dir = askpass
            .parent()
            .map(|parent| shell_quote(&parent.to_string_lossy()))
            .unwrap_or_else(|| script.clone());
        format!(
            "SSH_ASKPASS={script} SSH_ASKPASS_REQUIRE=force {}; rm -rf {dir}",
            self.shell_command()
        )
    }

    pub fn has_password(&self) -> bool {
        self.password
            .as_deref()
            .is_some_and(|password| !password.is_empty())
    }

    /// 列表里显示的 `user@host:port`。
    pub fn display_target(&self) -> String {
        let mut text = String::new();
        if let Some(user) = self
            .user
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
        {
            text.push_str(user);
            text.push('@');
        }
        text.push_str(&self.host);
        if let Some(port) = self.port
            && port != 22
        {
            text.push(':');
            text.push_str(&port.to_string());
        }
        text
    }

    /// 丢弃空字段和首尾空白，并在缺少名称时用主机名补上。
    pub fn normalize(&mut self) {
        self.name = self.name.trim().to_string();
        self.host = self.host.trim().to_string();
        self.user = trimmed(self.user.take());
        self.identity_file = trimmed(self.identity_file.take());
        self.group = trimmed(self.group.take());
        // 密码首尾的空格可能是密码的一部分，只丢弃完全为空的。
        self.password = self.password.take().filter(|password| !password.is_empty());
        self.id = self.id.trim().to_string();
        if self.name.is_empty() {
            self.name = self.host.clone();
        }
        if self.port == Some(22) {
            self.port = None;
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.name.is_empty() && !self.host.is_empty()
    }

    /// 给还没有 id 的条目（老文件、新建表单）补一个。
    pub fn ensure_id(&mut self, sequence: u64) {
        if self.id.is_empty() {
            self.id = new_connection_id(sequence);
        }
    }
}

/// 单调递增的标识：时间戳 + 进程内序号。
///
/// 不用随机数也不必引依赖——同一个列表里的条目必然先后创建，只要保证
/// 同一次运行内不重复、重启后也不易撞上即可。
fn new_connection_id(sequence: u64) -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{stamp:x}-{sequence}")
}

/// 保存后要把选中项定位回**可见列表中的位置**。
///
/// `visible_indices` 会按分组重排，所以底层数组下标不能直接当可见位置用：
/// 有分组时数组末尾的元素未必显示在最后。名称在列表里重复出现时取第一个，
/// 与点选只命中第一条的行为一致。
pub fn visible_position_of(book: &ConnectionBook, name: &str) -> Option<usize> {
    book.visible_indices("")
        .iter()
        .position(|&index| book.connections[index].name == name)
}

fn trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

/// POSIX 单引号包裹，内部单引号转成 `'"'"'`。
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// 把 `~/…` 展开成全路径，因为 ssh 的 `-i` 不接受 shell 展开。
fn expand_home(path: &str) -> String {
    let Some(rest) = path.strip_prefix("~/") else {
        return path.to_string();
    };
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home)
            .join(rest)
            .to_string_lossy()
            .into_owned(),
        None => path.to_string(),
    }
}

/// 写一个一次性的 askpass 脚本，里面只有这一条连接的密码。
///
/// 脚本以 0700 权限放在临时目录，由连接命令自己 `rm -rf` 整个目录；即使进程
/// 被强杀留下残骸，也只有当前用户可读。密码按单引号字面量写入，`$`、反引号、
/// 双引号都不会被 shell 展开。
pub fn write_askpass_script(connection: &SshConnection) -> std::io::Result<PathBuf> {
    let password = connection.password.as_deref().unwrap_or_default();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("con-askpass-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let script = dir.join("askpass");
    std::fs::write(
        &script,
        format!("#!/bin/sh\nprintf '%s\\n' {}\n", shell_quote(password)),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(script)
}

/// 把一条连接转成可直接写进终端的一行命令。
///
/// 有密码时顺带准备好 askpass 脚本。失败（临时目录不可用等）返回 `None`，
/// 由调用方决定怎么提示：直接连接要弹通知，分屏则退回本地 shell。
pub fn connect_command(connection: &SshConnection) -> Option<String> {
    if !connection.has_password() {
        return Some(connection.shell_command());
    }
    let script = write_askpass_script(connection)
        .inspect_err(|error| log::error!("connections: failed to write askpass script: {error}"))
        .ok()?;
    Some(connection.shell_command_with_askpass(&script))
}

/// 整份连接列表。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConnectionBook {
    pub connections: Vec<SshConnection>,
}

const FILE_NAME: &str = "connections.json";

impl ConnectionBook {
    pub fn path() -> PathBuf {
        con_paths::app_config_dir().join(FILE_NAME)
    }

    /// 读取已保存的连接。文件不存在或损坏时返回空列表——这份数据是纯附加
    /// 功能，读取失败不应阻止应用启动。
    pub fn load() -> Self {
        Self::load_from(Self::path())
    }

    pub fn load_from(path: impl AsRef<std::path::Path>) -> Self {
        let path = path.as_ref();
        let Ok(content) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_json::from_str::<Self>(&content) {
            Ok(mut book) => {
                book.normalize();
                book
            }
            Err(error) => {
                log::warn!("connections {:?} could not be parsed: {error}", path);
                Self::default()
            }
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(Self::path())
    }

    pub fn save_to(&self, path: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // 落盘前补齐 id，这样下一次 load 读到的就是稳定标识，不必再补发。
        // 只补 id，不跑完整的 normalize：那会连带丢弃缺名称/主机的条目，
        // 保存一次就悄悄删掉用户的数据。
        let mut book = self.clone();
        book.assign_missing_ids();
        let encoded = serde_json::to_string_pretty(&book)?;
        std::fs::write(path, encoded)?;
        Ok(())
    }

    /// 给缺 id 或 id 重复的条目补发新 id，其余字段一概不动。
    fn assign_missing_ids(&mut self) {
        let mut used: Vec<String> = Vec::with_capacity(self.connections.len());
        for (index, connection) in self.connections.iter_mut().enumerate() {
            if connection.id.is_empty() || used.contains(&connection.id) {
                connection.id = new_connection_id(index as u64);
            }
            used.push(connection.id.clone());
        }
    }

    fn normalize(&mut self) {
        self.connections.retain(|connection| {
            if connection.is_valid() {
                true
            } else {
                log::warn!(
                    "connections: dropping entry without name or host ({:?})",
                    connection.name
                );
                false
            }
        });
        for connection in &mut self.connections {
            connection.normalize();
        }

        // 老文件没有 id。补发时必须保证唯一，否则 ElementId 冲突会把两行的
        // 状态又串到一起——这正是要修的那个错位。
        self.assign_missing_ids();
    }

    /// 已有的分组名，按首次出现顺序，用于编辑器的下拉候选。
    pub fn group_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for connection in &self.connections {
            if let Some(group) = connection.group.as_deref()
                && !names.iter().any(|name| name == group)
            {
                names.push(group.to_string());
            }
        }
        names
    }

    /// 列表要渲染的下标：按分组排序，组内保持存储顺序，未分组的收尾。
    /// `query` 非空时只保留名称、主机或分组命中的项。
    pub fn visible_indices(&self, query: &str) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        let matches = |connection: &SshConnection| {
            query.is_empty()
                || connection.name.to_lowercase().contains(&query)
                || connection.host.to_lowercase().contains(&query)
                || connection
                    .group
                    .as_deref()
                    .is_some_and(|group| group.to_lowercase().contains(&query))
        };

        let mut groups: Vec<Option<&str>> = Vec::new();
        for connection in &self.connections {
            let group = connection.group.as_deref();
            if !groups.contains(&group) {
                groups.push(group);
            }
        }

        let named = groups.iter().filter(|group| group.is_some());
        let ungrouped = groups.iter().filter(|group| group.is_none());
        let mut indices = Vec::new();
        for group in named.chain(ungrouped) {
            for (index, connection) in self.connections.iter().enumerate() {
                if connection.group.as_deref() == *group && matches(connection) {
                    indices.push(index);
                }
            }
        }
        indices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(name: &str, host: &str) -> SshConnection {
        SshConnection {
            name: name.into(),
            host: host.into(),
            ..Default::default()
        }
    }

    /// 老文件没有 id 字段；读取时要补上且必须互不相同。
    #[test]
    fn legacy_entries_get_unique_ids_on_load() {
        let dir = std::env::temp_dir().join(format!(
            "con-connections-ids-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        std::fs::write(
            &path,
            r#"{"connections":[{"name":"a","host":"1.1.1.1"},{"name":"b","host":"2.2.2.2"}]}"#,
        )
        .unwrap();

        let book = ConnectionBook::load_from(&path);
        assert_eq!(book.connections.len(), 2);
        let first = book.connections[0].id.clone();
        let second = book.connections[1].id.clone();
        assert!(!first.is_empty() && !second.is_empty());
        assert_ne!(first, second, "两行的 ElementId 不能相同");

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 旧数据里若已有重复 id，也要被拆开。
    #[test]
    fn duplicate_ids_are_reissued_on_load() {
        let dir = std::env::temp_dir().join(format!(
            "con-connections-dup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        std::fs::write(
            &path,
            r#"{"connections":[{"id":"same","name":"a","host":"1.1.1.1"},{"id":"same","name":"b","host":"2.2.2.2"}]}"#,
        )
        .unwrap();

        let book = ConnectionBook::load_from(&path);
        assert_ne!(book.connections[0].id, book.connections[1].id);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn group_names_are_unique_and_keep_first_seen_order() {
        let mut a = connection("a", "1.1.1.1");
        a.group = Some("测试".into());
        let mut b = connection("b", "2.2.2.2");
        b.group = Some("生产".into());
        let mut c = connection("c", "3.3.3.3");
        c.group = Some("测试".into());
        let d = connection("d", "4.4.4.4");
        let book = ConnectionBook {
            connections: vec![a, b, c, d],
        };

        assert_eq!(book.group_names(), vec!["测试", "生产"]);
    }

    #[test]
    fn ssh_args_include_target_and_skip_default_port() {
        let mut c = connection("箱", "10.0.0.5");
        c.user = Some("root".into());
        assert_eq!(
            c.ssh_args(),
            ["-o", "StrictHostKeyChecking=accept-new", "root@10.0.0.5"]
        );

        c.port = Some(40022);
        assert_eq!(
            c.ssh_args(),
            [
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-p",
                "40022",
                "root@10.0.0.5"
            ]
        );
    }

    #[test]
    fn askpass_command_never_puts_the_password_on_the_command_line() {
        let mut c = connection("箱", "10.0.0.5");
        c.user = Some("root".into());
        c.password = Some("p@ss w'ord".into());
        assert!(c.has_password());

        let script = std::path::Path::new("/tmp/con-askpass-1/askpass");
        let command = c.shell_command_with_askpass(script);
        assert!(!command.contains("p@ss"));
        assert!(command.contains("SSH_ASKPASS_REQUIRE=force"));
        assert!(command.contains("'root@10.0.0.5'"));
        // 脚本连同它的一次性目录在 ssh 退出后一起删除。
        assert!(command.ends_with("rm -rf '/tmp/con-askpass-1'"));
    }

    /// `connect_command` 是 UI 唯一入口：有无密码都要能直接写进终端。
    #[test]
    fn connect_command_covers_password_and_key_auth() {
        let mut key_only = connection("箱", "10.0.0.5");
        key_only.user = Some("root".into());
        let plain = connect_command(&key_only).expect("无密码时不需要临时文件");
        assert_eq!(plain, key_only.shell_command());
        assert!(!plain.contains("SSH_ASKPASS"));

        let mut with_password = connection("箱", "10.0.0.5");
        with_password.password = Some("secret".into());
        let command = connect_command(&with_password).expect("应当写出 askpass 脚本");
        assert!(command.contains("SSH_ASKPASS_REQUIRE=force"));
        assert!(!command.contains("secret"), "密码不能出现在命令行");

        // 清理这次生成的临时目录。
        let dir = command
            .rsplit("rm -rf '")
            .next()
            .and_then(|rest| rest.strip_suffix('\''))
            .expect("命令末尾应当清理临时目录");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn askpass_script_writes_the_password_literally() {
        let mut c = connection("箱", "10.0.0.5");
        c.password = Some("a$b`c\"d'e".into());
        let script = write_askpass_script(&c).unwrap();
        let content = std::fs::read_to_string(&script).unwrap();
        assert_eq!(content, "#!/bin/sh\nprintf '%s\\n' 'a$b`c\"d'\"'\"'e'\n");

        let output = std::process::Command::new(&script).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout), "a$b`c\"d'e\n");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&script).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }

        std::fs::remove_dir_all(script.parent().unwrap()).unwrap();
    }

    #[test]
    fn normalize_keeps_password_spaces_but_drops_an_empty_one() {
        let mut c = connection("箱", "h");
        c.password = Some("  spaced  ".into());
        c.normalize();
        assert_eq!(c.password.as_deref(), Some("  spaced  "));

        c.password = Some(String::new());
        c.normalize();
        assert_eq!(c.password, None);
        assert!(!c.has_password());
    }

    #[test]
    fn identity_file_expands_home_and_is_passed_before_target() {
        let mut c = connection("箱", "example.com");
        c.identity_file = Some("~/.ssh/id_ed25519".into());
        let args = c.ssh_args();
        assert!(args.iter().any(|a| a == "-i"));
        let key = args.iter().position(|a| a == "-i").unwrap() + 1;
        assert!(args[key].ends_with("/.ssh/id_ed25519"));
        assert!(!args[key].starts_with('~'));
        assert_eq!(args.last().unwrap(), "example.com");
    }

    #[test]
    fn normalize_fills_missing_name_and_drops_blank_fields() {
        let mut c = connection("  ", " 10.0.0.5 ");
        c.user = Some("   ".into());
        c.port = Some(22);
        c.normalize();
        assert_eq!(c.name, "10.0.0.5");
        assert_eq!(c.host, "10.0.0.5");
        assert_eq!(c.user, None);
        assert_eq!(c.port, None);
    }

    #[test]
    fn book_round_trips_and_keeps_order() {
        let dir = std::env::temp_dir().join(format!(
            "con-connections-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);

        let book = ConnectionBook {
            connections: vec![connection("b", "2.2.2.2"), connection("a", "1.1.1.1")],
        };
        book.save_to(&path).unwrap();
        let loaded = ConnectionBook::load_from(&path);
        // save_to 会补 id，所以内存对象与读回的内容在 id 上不同；其余字段
        // 与顺序都必须原样保留。
        assert_eq!(loaded.connections.len(), book.connections.len());
        for (loaded, original) in loaded.connections.iter().zip(&book.connections) {
            assert_eq!(loaded.name, original.name);
            assert_eq!(loaded.host, original.host);
            assert!(!loaded.id.is_empty());
        }
        assert_eq!(loaded.connections[0].name, "b");

        // 再存一次必须稳定：id 不该每次写盘都变。
        let reloaded_ids: Vec<_> = loaded.connections.iter().map(|c| c.id.clone()).collect();
        loaded.save_to(&path).unwrap();
        let again: Vec<_> = ConnectionBook::load_from(&path)
            .connections
            .iter()
            .map(|c| c.id.clone())
            .collect();
        assert_eq!(reloaded_ids, again);

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 保存只补 id，不顺手清理条目。缺名称/主机的项在加载时才丢弃；
    /// 若保存时也跑 normalize，一次误写就会把用户的数据从磁盘上抹掉。
    #[test]
    fn save_assigns_ids_without_dropping_incomplete_entries() {
        let dir = std::env::temp_dir().join(format!(
            "con-connections-keep-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);

        let book = ConnectionBook {
            connections: vec![connection("正常的", "1.1.1.1"), connection("", "2.2.2.2")],
        };
        book.save_to(&path).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: ConnectionBook = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.connections.len(), 2, "保存不该丢弃条目");
        assert!(parsed.connections.iter().all(|c| !c.id.is_empty()));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_or_corrupt_file_yields_empty_book() {
        let dir = std::env::temp_dir().join(format!(
            "con-connections-bad-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        assert!(ConnectionBook::load_from(&path).connections.is_empty());

        std::fs::write(&path, "{not json").unwrap();
        assert!(ConnectionBook::load_from(&path).connections.is_empty());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn shell_command_quotes_every_argument() {
        let mut c = connection("箱", "10.0.0.5");
        c.user = Some("root".into());
        c.port = Some(40022);
        assert_eq!(
            c.shell_command(),
            "command ssh '-o' 'StrictHostKeyChecking=accept-new' '-p' '40022' 'root@10.0.0.5'"
        );

        // 用户在名称/主机里敲进 shell 元字符时不能变成额外命令。
        let mut c = connection("箱", "a;rm -rf $HOME");
        c.user = None;
        assert_eq!(
            c.shell_command(),
            "command ssh '-o' 'StrictHostKeyChecking=accept-new' 'a;rm -rf $HOME'"
        );
    }

    /// 用户 rc 里把 `ssh` 定义成函数时（Ghostty shell 集成就这么做），
    /// 连接命令必须绕开它——否则函数里的包装可能指向不存在的二进制。
    #[test]
    fn shell_command_bypasses_a_user_defined_ssh_function() {
        let mut c = connection("箱", "host");
        c.password = Some("pw".into());
        let script = write_askpass_script(&c).unwrap();
        let command = c.shell_command_with_askpass(&script);
        std::fs::remove_dir_all(script.parent().unwrap()).unwrap();

        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("ssh() {{ echo HIJACKED; }}; {command}"))
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.contains("HIJACKED"), "{stdout}");
    }

    #[test]
    fn visible_indices_group_named_entries_before_ungrouped() {
        let mut a = connection("a", "1.1.1.1");
        a.group = Some("生产".into());
        let c = connection("c", "3.3.3.3");
        let mut b = connection("b", "2.2.2.2");
        b.group = Some("生产".into());
        let mut d = connection("d", "4.4.4.4");
        d.group = Some("测试".into());
        let book = ConnectionBook {
            connections: vec![a, c, b, d],
        };

        // 未分组被排到最后，同组的两条保持存储顺序。
        assert_eq!(book.visible_indices(""), vec![0, 2, 3, 1]);
    }

    /// 有分组时数组下标和可见位置不同：未分组条目总被排到最后，
    /// 所以「数组最后一个元素」未必在可见列表末尾。
    #[test]
    fn visible_position_differs_from_array_index_when_grouped() {
        let ungrouped = connection("未分组的", "0.0.0.0");
        let mut grouped = connection("分组里的", "1.1.1.1");
        grouped.group = Some("生产".into());
        let book = ConnectionBook {
            connections: vec![ungrouped, grouped],
        };

        // 数组下标 1 的「分组里的」显示在第一位；若直接把数组长度减一当
        // 可见位置，会选中排在它后面的「未分组的」。
        assert_eq!(visible_position_of(&book, "分组里的"), Some(0));
        assert_eq!(visible_position_of(&book, "未分组的"), Some(1));
        assert_eq!(visible_position_of(&book, "不存在"), None);
    }

    #[test]
    fn visible_indices_filter_by_name_host_or_group() {
        let mut a = connection("香港-博客", "1.1.1.1");
        a.group = Some("生产".into());
        let b = connection("内网", "10.0.0.9");
        let book = ConnectionBook {
            connections: vec![a, b],
        };

        assert_eq!(book.visible_indices("香港"), vec![0]);
        assert_eq!(book.visible_indices("10.0.0"), vec![1]);
        assert_eq!(book.visible_indices("生产"), vec![0]);
        assert_eq!(book.visible_indices("  生产  "), vec![0]);
        assert!(book.visible_indices("不存在").is_empty());
    }
}
