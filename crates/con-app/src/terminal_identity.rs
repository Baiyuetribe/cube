//! Display-only identities derived from retained process observations.
//! Never infer an application from terminal output or use this for agent control.

pub(crate) const TERMINAL: &str = "phosphor/terminal.svg";

fn program(name: &str) -> Option<(&'static str, u8)> {
    let name = name.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    Some(match name {
        "nvim" | "neovim" => ("program:neovim", 3),
        "vim" | "vi" | "view" | "gvim" => ("program:vim", 3),
        "emacs" | "emacsclient" => ("program:emacs", 3),
        "nano" => ("program:nano", 3),
        "pico" | "micro" | "hx" | "helix" | "kak" | "kakoune" => ("phosphor/code.svg", 3),
        "python" | "pypy" | "pypy3" | "ipython" | "ipython3" => ("program:python", 2),
        "node" | "nodejs" => ("program:node", 2),
        "npm" | "npx" | "pnpm" | "yarn" | "bun" | "deno" => ("program:javascript", 2),
        "ruby" | "irb" | "pry" | "rails" | "rake" | "bundle" => ("program:ruby", 2),
        "cargo" | "rustc" | "rustup" => ("program:rust", 2),
        "go" | "gopls" => ("program:go", 2),
        "java" | "jshell" | "javac" | "gradle" | "mvn" => ("program:java", 2),
        "lua" | "luajit" => ("program:lua", 2),
        "zig" => ("program:zig", 2),
        "docker" | "podman" => ("program:container", 1),
        "kubectl" | "helm" => ("program:kubernetes", 1),
        "terraform" | "tofu" => ("program:terraform", 1),
        "sqlite3" => ("program:sqlite", 1),
        "psql" | "mysql" | "mariadb" | "mongosh" => ("program:database", 1),
        "redis-cli" => ("program:redis", 1),
        "git" | "lazygit" | "tig" | "gh" => ("phosphor/file-code.svg", 1),
        "ssh" | "mosh" | "sftp" => ("phosphor/globe.svg", 1),
        "htop" | "top" | "btop" | "btm" | "k9s" | "glances" | "nvtop" => ("phosphor/pulse.svg", 1),
        "less" | "more" | "man" | "bat" => ("phosphor/book-open.svg", 1),
        _ if name.strip_prefix("python").is_some_and(|version| {
            !version.is_empty()
                && version
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        }) =>
        {
            ("program:python", 2)
        }
        _ => return None,
    })
}

pub(crate) fn from_processes<'a>(names: impl IntoIterator<Item = &'a str>) -> &'static str {
    let mut selected = None;
    let mut ambiguous = false;
    for (icon, rank) in names.into_iter().filter_map(program) {
        match selected {
            None => {
                selected = Some((icon, rank));
                ambiguous = false;
            }
            Some((_, previous)) if rank > previous => {
                selected = Some((icon, rank));
                ambiguous = false;
            }
            Some((old, previous)) if rank == previous && icon != old => ambiguous = true,
            _ => {}
        }
    }
    selected
        .filter(|_| !ambiguous)
        .map_or(TERMINAL, |(icon, _)| icon)
}

/// Existing bundled Nerd Font identities, also used by the file tree. No
/// installed terminal font, network lookup or new icon pack is required.
pub(crate) fn glyph(icon: &str) -> Option<char> {
    Some(match icon {
        "program:neovim" => '\u{e6ae}',
        "program:vim" => '\u{e62b}',
        "program:emacs" => '\u{e632}',
        "program:nano" => '\u{e838}',
        "program:python" => '\u{e606}',
        "program:node" => '\u{e719}',
        "program:javascript" => '\u{e60c}',
        "program:ruby" => '\u{e605}',
        "program:rust" => '\u{e68b}',
        "program:go" => '\u{e627}',
        "program:java" => '\u{e66d}',
        "program:lua" => '\u{e620}',
        "program:zig" => '\u{e6a9}',
        "program:container" => '\u{e7b0}',
        "program:kubernetes" => '\u{e81d}',
        "program:terraform" => '\u{e8bd}',
        "program:sqlite" => '\u{e7c4}',
        "program:database" => '\u{e64d}',
        "program:redis" => '\u{e76d}',
        _ => return None,
    })
}

pub(crate) fn color(icon: &str) -> gpui::Hsla {
    gpui::rgb(match icon {
        "program:vim" | "program:neovim" => 0x328d5b,
        "program:emacs" | "program:terraform" => 0x8963b4,
        "program:python" | "program:nano" => 0x3d79ac,
        "program:node" => 0x408b46,
        "program:javascript" => 0xa58024,
        "program:ruby" | "program:redis" => 0xbf5158,
        "program:rust" | "program:java" => 0xb16f45,
        "program:go" => 0x258d9e,
        "program:zig" => 0xad7b35,
        _ => 0x4d83b6,
    })
    .into()
}

/// Match the optical inset of the bitmap identities, not the font's advance
/// cell. Parse the bundled face once; this does not inspect installed fonts.
pub(crate) fn glyph_geometry(
    glyph: char,
    slot: gpui::Pixels,
) -> Option<(gpui::Pixels, gpui::Bounds<gpui::Pixels>)> {
    static FACE: std::sync::OnceLock<ttf_parser::Face<'static>> = std::sync::OnceLock::new();
    let face = FACE.get_or_init(|| {
        ttf_parser::Face::parse(
            include_bytes!("../../../assets/fonts/IoskeleyMono-Icons.ttf"),
            0,
        )
        .expect("bundled icon font")
    });
    let bounds = face.glyph_bounding_box(face.glyph_index(glyph)?)?;
    let extent = f32::from(bounds.width().max(bounds.height()));
    if extent <= 0.0 {
        return None;
    }
    let scale = slot.as_f32() * 0.80 / extent;
    Some((
        gpui::px(f32::from(face.units_per_em()) * scale),
        gpui::Bounds::new(
            gpui::point(
                gpui::px(f32::from(bounds.x_min) * scale),
                gpui::px(f32::from(bounds.y_min) * scale),
            ),
            gpui::size(
                gpui::px(f32::from(bounds.width()) * scale),
                gpui::px(f32::from(bounds.height()) * scale),
            ),
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_aliases_are_exact_and_platform_independent() {
        for (name, icon) in [
            ("/opt/homebrew/bin/nvim", "program:neovim"),
            (r"C:\Tools\VIM.EXE", "program:vim"),
            ("python3.12", "program:python"),
            ("node.exe", "program:node"),
            ("irb", "program:ruby"),
            ("emacsclient", "program:emacs"),
            ("nano", "program:nano"),
        ] {
            assert_eq!(from_processes([name]), icon);
        }
        for name in [
            "python-notes",
            "node_modules",
            "nvim README.md",
            "python3.bad",
            "zsh",
        ] {
            assert_eq!(from_processes([name]), TERMINAL);
        }
    }

    #[test]
    fn editors_outrank_plugin_runtimes_and_conflicts_stay_generic() {
        assert_eq!(
            from_processes(["node", "nvim", "python3"]),
            "program:neovim"
        );
        assert_eq!(
            from_processes(["python3", "nvim", "node"]),
            "program:neovim"
        );
        assert_eq!(from_processes(["python", "node"]), TERMINAL);
        assert_eq!(from_processes(["python", "python3.12"]), "program:python");
        assert_eq!(from_processes([]), TERMINAL);
    }

    #[test]
    fn bundled_font_contains_every_program_identity() {
        let font = ttf_parser::Face::parse(
            include_bytes!("../../../assets/fonts/IoskeleyMono-Icons.ttf"),
            0,
        )
        .unwrap();
        for name in [
            "neovim",
            "vim",
            "emacs",
            "python",
            "node",
            "ruby",
            "rust",
            "go",
            "java",
            "lua",
            "zig",
            "container",
            "nano",
            "kubernetes",
            "terraform",
            "sqlite",
            "database",
            "redis",
            "javascript",
        ] {
            let glyph = glyph(&format!("program:{name}")).unwrap();
            for size in [12.0, 18.0, 24.0] {
                let (_, ink) = glyph_geometry(glyph, gpui::px(size)).unwrap();
                assert!((ink.size.width.max(ink.size.height).as_f32() - size * 0.8).abs() < 0.001);
            }
            let bounds = font
                .glyph_bounding_box(font.glyph_index(glyph).unwrap())
                .unwrap();
            assert!(
                bounds.width() > 0 && bounds.height() > 0,
                "missing {name} glyph"
            );
        }
    }
}
