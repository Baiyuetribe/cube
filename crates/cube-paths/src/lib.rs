//! Shared per-user app paths.
//!
//! Keep this policy centralized so config, session, auth, themes, and future
//! storage paths do not drift.

use std::path::PathBuf;

/// `CUBE` is not a reserved DOS device name, so unlike the project's previous
/// `con` name there is no per-platform split here.
pub const APP_DIR_NAME: &str = "cube";
pub const CONFIG_FILE_NAME: &str = "cube.conf";

/// Directory name used by the previous `con` build.
///
/// Only read during the one-time migration when `cube` has no config yet;
/// after that it is never touched again.
pub const LEGACY_APP_DIR_NAME: &str = "con";

/// Config file name used by the previous `con` build.
pub const LEGACY_CONFIG_FILE_NAME: &str = "con.conf";

pub fn app_config_dir() -> PathBuf {
    dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_DIR_NAME)
}

pub fn app_data_dir() -> PathBuf {
    dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")))
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_DIR_NAME)
}

pub fn app_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_DIR_NAME)
}

/// Per-user override for Cube's freedesktop launcher icon. This is separate
/// from private application data so desktop environments can discover it.
#[cfg(target_os = "linux")]
pub fn launcher_icon_path() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("icons/hicolor/256x256/apps/co.nowledge.cube.png"))
}

pub fn config_file() -> PathBuf {
    app_config_dir().join(CONFIG_FILE_NAME)
}

/// The former TOML configuration. It is only used as a migration source.
pub fn legacy_config_file() -> PathBuf {
    app_config_dir().join("config.toml")
}

pub fn default_global_skills_path() -> String {
    format!("~/.config/{APP_DIR_NAME}/skills")
}

pub fn user_themes_dir() -> PathBuf {
    app_config_dir().join("themes")
}

pub fn runtime_dir() -> PathBuf {
    #[cfg(unix)]
    {
        if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
            let path = PathBuf::from(dir);
            if path.is_dir() {
                return path;
            }
        }
        std::env::temp_dir()
    }
    #[cfg(not(unix))]
    {
        std::env::temp_dir()
    }
}

/// 一次性迁移：把旧 `con` 数据目录复制到新的 `cube` 目录。
///
/// 只在 `cube` 目录还不存在时执行，复制完保留旧目录（不删，便于回退）。
/// 失败时返回错误由调用方记录日志——迁移失败不该阻止应用启动，
/// 用户会看到一份空配置而不是崩溃。
pub fn migrate_legacy_data_dir() -> Result<Option<PathBuf>, std::io::Error> {
    let Some(target) = dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
        .map(|dir| dir.join(APP_DIR_NAME))
    else {
        return Ok(None);
    };
    migrate_dir_if_absent(&target, LEGACY_APP_DIR_NAME)
}

/// 把 `legacy_name` 目录复制到 `target`，`target` 已存在则跳过。
///
/// 返回被复制的源目录（没搬动任何东西时为 `None`）。
pub fn migrate_dir_if_absent(
    target: &std::path::Path,
    legacy_name: &str,
) -> Result<Option<PathBuf>, std::io::Error> {
    if target.exists() {
        return Ok(None);
    }
    let Some(parent) = target.parent() else {
        return Ok(None);
    };
    let source = parent.join(legacy_name);
    if !source.is_dir() {
        return Ok(None);
    }
    copy_dir_all(&source, target)?;
    // 旧配置文件名也要跟着改，否则新目录里找不到配置。
    let legacy_config = target.join(LEGACY_CONFIG_FILE_NAME);
    if legacy_config.is_file() {
        std::fs::rename(&legacy_config, target.join(CONFIG_FILE_NAME))?;
    }
    Ok(Some(source))
}

/// 递归复制目录。符号链接按链接本身复制，不跟随——跟随可能把
/// 目录树外的内容拉进来，或陷入循环。
fn copy_dir_all(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            copy_symlink(&from, &to)?;
        } else if file_type.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn copy_symlink(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    let target = std::fs::read_link(from)?;
    std::os::unix::fs::symlink(target, to)
}

#[cfg(not(unix))]
fn copy_symlink(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    // Windows 上创建符号链接需要额外权限，退化为复制内容。
    std::fs::copy(from, to).map(|_| ())
}

/// Returns true if running inside a Flatpak container.
pub fn is_flatpak() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new("/.flatpak-info").exists()
            || std::env::var("FLATPAK_ID").is_ok()
            || std::env::var("container").is_ok_and(|v| v == "flatpak")
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Constructs a `std::process::Command` that executes on the host system.
/// If running inside a Flatpak container, wraps the command with `flatpak-spawn --host`.
pub fn host_command(program: &str) -> std::process::Command {
    if is_flatpak() {
        let mut cmd = std::process::Command::new("flatpak-spawn");
        cmd.arg("--host");
        cmd.arg("--unset-env=FLATPAK_ID");
        cmd.arg("--unset-env=container");
        cmd.arg(program);
        cmd
    } else {
        std::process::Command::new(program)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        APP_DIR_NAME, CONFIG_FILE_NAME, LEGACY_APP_DIR_NAME, LEGACY_CONFIG_FILE_NAME,
        app_config_dir, app_data_dir, config_file, default_global_skills_path, host_command,
        is_flatpak, legacy_config_file, migrate_dir_if_absent, user_themes_dir,
    };

    #[test]
    fn migration_copies_legacy_dir_once_and_renames_config() {
        let root = std::env::temp_dir().join(format!(
            "cube-paths-migrate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let legacy = root.join(LEGACY_APP_DIR_NAME);
        std::fs::create_dir_all(legacy.join("themes")).unwrap();
        std::fs::write(legacy.join(LEGACY_CONFIG_FILE_NAME), "con.version = 1\n").unwrap();
        std::fs::write(legacy.join("session.json"), "{}").unwrap();
        std::fs::write(legacy.join("themes").join("mine.json"), "{}").unwrap();

        let target = root.join(APP_DIR_NAME);
        let moved = migrate_dir_if_absent(&target, LEGACY_APP_DIR_NAME).unwrap();
        assert_eq!(moved.as_deref(), Some(legacy.as_path()));

        // 配置文件改名，子目录内容一并复制。
        assert!(target.join(CONFIG_FILE_NAME).is_file());
        assert!(!target.join(LEGACY_CONFIG_FILE_NAME).exists());
        assert!(target.join("session.json").is_file());
        assert!(target.join("themes").join("mine.json").is_file());

        // 旧目录保留，便于回退。
        assert!(legacy.is_dir());

        // 第二次调用不再搬动任何东西。
        assert!(
            migrate_dir_if_absent(&target, LEGACY_APP_DIR_NAME)
                .unwrap()
                .is_none()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn app_paths_use_app_dir_name() {
        assert!(app_config_dir().ends_with(APP_DIR_NAME));
        assert!(app_data_dir().ends_with(APP_DIR_NAME));
        assert!(
            config_file()
                .ends_with(std::path::Path::new(APP_DIR_NAME).join(super::CONFIG_FILE_NAME))
        );
        assert!(
            legacy_config_file().ends_with(std::path::Path::new(APP_DIR_NAME).join("config.toml"))
        );
        assert!(user_themes_dir().ends_with(std::path::Path::new(APP_DIR_NAME).join("themes")));
    }

    #[test]
    fn default_skill_path_uses_app_dir_name() {
        // `CUBE` 不是保留名，所有平台用同一个目录。
        assert_eq!(default_global_skills_path(), "~/.config/cube/skills");
    }

    #[test]
    fn host_command_returns_command() {
        let cmd = host_command("git");
        if is_flatpak() {
            assert_eq!(cmd.get_program(), "flatpak-spawn");
        } else {
            assert_eq!(cmd.get_program(), "git");
        }
    }
}
