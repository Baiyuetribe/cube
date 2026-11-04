use std::path::Path;

#[cfg(target_family = "unix")]
use std::os::unix::fs as unix_fs;

pub fn ensure_cli_shim() {
    if let Err(err) = ensure_cli_shim_inner() {
        log::warn!("cube-cli shim: {err}");
    }
}

fn ensure_cli_shim_inner() -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|err| format!("could not resolve current executable: {err}"))?;
    let Some(macos_dir) = exe.parent() else {
        return Ok(());
    };
    if !is_app_bundle_macos_dir(macos_dir) {
        return Ok(());
    }

    let cli = macos_dir.join("cube-cli");
    if !cli.is_file() {
        return Ok(());
    }
    if !is_installed_cube_app_cli_target(&cli) {
        log::info!(
            "cube-cli shim: not linking from transient app bundle at {}",
            cli.display()
        );
        return Ok(());
    }

    let Some(home) = dirs::home_dir() else {
        return Ok(());
    };
    let bin_dir = home.join(".local/bin");
    let link = bin_dir.join("cube-cli");

    match std::fs::symlink_metadata(&link) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let target = std::fs::read_link(&link)
                .map_err(|err| format!("could not inspect {}: {err}", link.display()))?;
            if target == cli {
                return Ok(());
            }
            if !is_installed_cube_app_cli_target(&target) {
                log::info!(
                    "cube-cli shim: preserving user-managed symlink at {} -> {}",
                    link.display(),
                    target.display()
                );
                return Ok(());
            }
            std::fs::remove_file(&link)
                .map_err(|err| format!("could not replace {}: {err}", link.display()))?;
        }
        Ok(_) => {
            log::info!(
                "cube-cli shim: preserving user-managed file at {}",
                link.display()
            );
            return Ok(());
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(format!("could not inspect {}: {err}", link.display()));
        }
    }

    std::fs::create_dir_all(&bin_dir)
        .map_err(|err| format!("could not create {}: {err}", bin_dir.display()))?;
    symlink_file(&cli, &link).map_err(|err| {
        format!(
            "could not link {} -> {}: {err}",
            link.display(),
            cli.display()
        )
    })?;
    log::info!(
        "cube-cli shim: linked {} -> {}",
        link.display(),
        cli.display()
    );
    Ok(())
}

fn is_app_bundle_macos_dir(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("MacOS")
        && path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            == Some("Contents")
        && path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::extension)
            .and_then(|ext| ext.to_str())
            == Some("app")
}

fn is_cube_app_cli_target(target: &Path) -> bool {
    if target.file_name().and_then(|name| name.to_str()) != Some("cube-cli") {
        return false;
    }

    let Some(macos_dir) = target.parent() else {
        return false;
    };
    if !is_app_bundle_macos_dir(macos_dir) {
        return false;
    }

    // Keep ownership intentionally narrow. Sparkle/manual installs should
    // repair stale links to Cube's own app bundles, but we must not replace a
    // user-managed symlink to another app that happens to ship `cube-cli`.
    matches!(
        macos_dir
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|name| name.to_str()),
        Some("cube.app" | "cube Beta.app" | "cube Dev.app")
    )
}

fn is_installed_cube_app_cli_target(target: &Path) -> bool {
    if !is_cube_app_cli_target(target) {
        return false;
    }

    let Some(app_bundle) = target
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
    else {
        return false;
    };

    // Only app bundles in normal install locations are considered managed.
    // A mounted DMG or copied test bundle with the same app name must not steal
    // ~/.local/bin/cube-cli away from the installed app.
    app_bundle.starts_with("/Applications")
        || dirs::home_dir()
            .map(|home| app_bundle.starts_with(home.join("Applications")))
            .unwrap_or(false)
}

#[cfg(target_family = "unix")]
fn symlink_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    unix_fs::symlink(src, dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_bundle_macos_dir_is_recognized() {
        assert!(is_app_bundle_macos_dir(Path::new(
            "/Applications/cube Beta.app/Contents/MacOS"
        )));
        assert!(!is_app_bundle_macos_dir(Path::new("/tmp/target/release")));
    }

    #[test]
    fn only_cube_app_cli_symlinks_are_managed() {
        assert!(is_cube_app_cli_target(Path::new(
            "/Applications/cube Beta.app/Contents/MacOS/cube-cli"
        )));
        assert!(is_cube_app_cli_target(Path::new(
            "/Applications/cube Dev.app/Contents/MacOS/cube-cli"
        )));
        assert!(!is_cube_app_cli_target(Path::new(
            "/opt/homebrew/bin/cube-cli"
        )));
        assert!(!is_cube_app_cli_target(Path::new(
            "/Applications/Other.app/Contents/MacOS/other-cli"
        )));
        assert!(!is_cube_app_cli_target(Path::new(
            "/Applications/Other.app/Contents/MacOS/cube-cli"
        )));
    }

    #[test]
    fn only_installed_cube_app_cli_symlinks_are_managed() {
        assert!(is_installed_cube_app_cli_target(Path::new(
            "/Applications/cube Beta.app/Contents/MacOS/cube-cli"
        )));
        assert!(is_installed_cube_app_cli_target(Path::new(
            "/Applications/cube Dev.app/Contents/MacOS/cube-cli"
        )));
        assert!(!is_installed_cube_app_cli_target(Path::new(
            "/Volumes/cube Beta/cube Beta.app/Contents/MacOS/cube-cli"
        )));
        assert!(!is_installed_cube_app_cli_target(Path::new(
            "/tmp/cube Beta.app/Contents/MacOS/cube-cli"
        )));
    }
}
