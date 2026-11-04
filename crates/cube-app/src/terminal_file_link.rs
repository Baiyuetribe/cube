//! Routes plain terminal links separately from the OSC 8 URL policy.
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerminalFileLink {
    pub path: PathBuf,
    /// One-based line and character column.
    pub position: Option<(usize, usize)>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PlainLink {
    File(TerminalFileLink),
    Url(String),
    Ignore,
}

pub(crate) fn resolve_plain_link(raw: &str, cwd: Option<&Path>) -> PlainLink {
    if raw.is_empty() || raw.chars().any(char::is_control) {
        return PlainLink::Ignore;
    }
    // URL ports and fragments must never be mistaken for source locations.
    if raw.contains("://") || raw.starts_with("file:") {
        let Ok(url) = url::Url::parse(raw) else {
            return PlainLink::Ignore;
        };
        if url.scheme() != "file" {
            return PlainLink::Url(raw.into());
        }
        let Ok(path) = url.to_file_path() else {
            return PlainLink::Ignore;
        };
        return local_file(path, None);
    }
    // Keep known non-hierarchical protocols distinct even if a same-named
    // local file exists (for example a file named `tel`).
    let parsed_url = url::Url::parse(raw).ok();
    if parsed_url
        .as_ref()
        .is_some_and(|url| matches!(url.scheme(), "mailto" | "tel" | "ssh" | "magnet" | "news"))
    {
        return PlainLink::Url(raw.into());
    }
    // Literal filenames containing colons take precedence over source suffixes.
    if let Some(path) = resolve_path(raw, cwd).filter(|path| path.exists()) {
        return local_file(path, None);
    }
    let (name, position) = split_position(raw);
    if position.is_some() {
        if let Some(path) = resolve_path(name, cwd).filter(|path| path.is_file()) {
            return local_file(path, position);
        }
        // A missing source location should not become an unknown URL scheme.
        return PlainLink::Ignore;
    }
    if parsed_url.is_some() {
        return PlainLink::Url(raw.into());
    }
    PlainLink::Ignore
}

fn resolve_path(raw: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    if Path::new(raw).is_absolute() {
        Some(PathBuf::from(raw))
    } else if let Some(rest) = raw.strip_prefix("~/") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(rest))
    } else {
        cwd.filter(|path| path.is_absolute())
            .map(|cwd| cwd.join(raw))
    }
}

fn local_file(path: PathBuf, position: Option<(usize, usize)>) -> PlainLink {
    let Ok(metadata) = path.metadata() else {
        return PlainLink::Ignore;
    };
    // Do not read devices, pipes, or sockets while handling a click.
    if !metadata.is_file() && !metadata.is_dir() {
        return PlainLink::Ignore;
    }
    let Ok(path) = path.canonicalize() else {
        return PlainLink::Ignore;
    };
    if metadata.is_file() && crate::editor_syntax::is_image_path(&path) {
        return PlainLink::File(TerminalFileLink {
            path,
            position: None,
        });
    }
    if metadata.is_file() && is_editor_text(&path, metadata.len()) {
        return PlainLink::File(TerminalFileLink { path, position });
    }
    // Launch Services needs an encoded file URL, not a bare filesystem path.
    match url::Url::from_file_path(&path) {
        Ok(url) => PlainLink::Url(url.into()),
        Err(_) => PlainLink::Ignore,
    }
}

fn is_editor_text(path: &Path, size: u64) -> bool {
    // The editor currently loads the entire file. Keep large files out of this
    // path and leave them to the user's associated application.
    const MAX_EDITOR_BYTES: u64 = 8 * 1024 * 1024;
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    // Some document formats (e.g. PDF and RTF) can start with valid text but
    // should be displayed by their associated document viewer.
    if size > MAX_EDITOR_BYTES
        || matches!(
            extension.as_str(),
            "pdf"
                | "ppt"
                | "pptx"
                | "pptm"
                | "pps"
                | "ppsx"
                | "pot"
                | "potx"
                | "doc"
                | "docx"
                | "docm"
                | "xls"
                | "xlsx"
                | "xlsm"
                | "odt"
                | "ods"
                | "odp"
                | "rtf"
                | "pages"
                | "numbers"
                | "key"
                | "epub"
                | "zip"
                | "gz"
                | "bz2"
                | "xz"
                | "7z"
                | "rar"
                | "tar"
                | "mp3"
                | "wav"
                | "mp4"
                | "mov"
                | "mkv"
                | "dmg"
                | "pkg"
        )
    {
        return false;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    // Validate the whole bounded file: binary data may occur after a text header.
    // Callers run filesystem classification on the background executor.
    let mut sample = Vec::with_capacity(size as usize);
    if file
        .take(MAX_EDITOR_BYTES + 1)
        .read_to_end(&mut sample)
        .is_err()
        || sample.len() as u64 > MAX_EDITOR_BYTES
    {
        return false;
    }
    !sample
        .iter()
        .any(|byte| *byte < 32 && !matches!(*byte, b'\n' | b'\r' | b'\t' | 12))
        && std::str::from_utf8(&sample).is_ok()
}

fn split_position(raw: &str) -> (&str, Option<(usize, usize)>) {
    let Some((prefix, last)) = raw.rsplit_once(':') else {
        return (raw, None);
    };
    let Ok(last) = last.parse::<usize>() else {
        return (raw, None);
    };
    if let Some((path, line)) = prefix.rsplit_once(':')
        && let Ok(line) = line.parse::<usize>()
    {
        return (path, Some((line.max(1), last.max(1))));
    }
    (prefix, Some((last.max(1), 1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_file_links_resolve_paths_and_locations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.rs");
        std::fs::write(&path, "// test").unwrap();
        let canonical = path.canonicalize().unwrap();
        for (raw, position) in [
            (path.display().to_string(), None),
            ("./sample.rs".into(), None),
            ("sample.rs:20".into(), Some((20, 1))),
            ("./sample.rs:30:5".into(), Some((30, 5))),
            ("./sample.rs:0:0".into(), Some((1, 1))),
            (url::Url::from_file_path(&path).unwrap().to_string(), None),
        ] {
            assert_eq!(
                resolve_plain_link(&raw, Some(dir.path())),
                PlainLink::File(TerminalFileLink {
                    path: canonical.clone(),
                    position,
                })
            );
        }
        for raw in [
            "./missing.rs:10",
            "./sample.rs\n",
            "file://remote.test/tmp/sample.rs",
        ] {
            assert_eq!(resolve_plain_link(raw, Some(dir.path())), PlainLink::Ignore);
        }
        assert_eq!(resolve_plain_link("./sample.rs", None), PlainLink::Ignore);
        let literal = dir.path().join("sample.rs:20");
        std::fs::write(&literal, "literal").unwrap();
        assert_eq!(
            resolve_plain_link("sample.rs:20", Some(dir.path())),
            PlainLink::File(TerminalFileLink {
                path: literal.canonicalize().unwrap(),
                position: None,
            })
        );
    }

    #[test]
    fn terminal_file_types_choose_editor_or_encoded_system_url() {
        let dir = tempfile::tempdir().unwrap();
        for (name, contents, internal) in [
            ("notes.txt", b"hello".as_slice(), true),
            ("README", b"hello".as_slice(), true),
            (".env", b"NAME=value".as_slice(), true),
            ("custom.unknown", b"plain text".as_slice(), true),
            ("photo.PNG", b"image fixture".as_slice(), true),
            ("deck 中文 #1.pptx", b"document fixture".as_slice(), false),
            ("paper.pdf", b"%PDF-1.7".as_slice(), false),
            ("document.rtf", b"{rtf}".as_slice(), false),
            ("binary.rs", b"\x00\x01\xff".as_slice(), false),
            ("utf16.txt", b"\xff\xfea\x00".as_slice(), false),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, contents).unwrap();
            let path = path.canonicalize().unwrap();
            let actual = resolve_plain_link(path.to_str().unwrap(), None);
            if internal {
                assert!(matches!(actual, PlainLink::File(_)), "{name}: {actual:?}");
            } else {
                assert_eq!(
                    actual,
                    PlainLink::Url(url::Url::from_file_path(&path).unwrap().into())
                );
            }
        }
        let folder = dir.path().canonicalize().unwrap();
        assert_eq!(
            resolve_plain_link("./", Some(&folder)),
            PlainLink::Url(url::Url::from_file_path(&folder).unwrap().into())
        );
        let large = folder.join("large.log");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(9 * 1024 * 1024)
            .unwrap();
        assert!(matches!(
            resolve_plain_link(large.to_str().unwrap(), None),
            PlainLink::Url(_)
        ));
        let image = folder.join("photo.PNG");
        assert_eq!(
            resolve_plain_link("./photo.PNG:20:5", Some(&folder)),
            PlainLink::File(TerminalFileLink {
                path: image,
                position: None
            })
        );
        assert!(matches!(
            resolve_plain_link("./paper.pdf:20", Some(&folder)),
            PlainLink::Url(_)
        ));
    }

    #[test]
    fn text_detection_checks_beyond_the_initial_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unicode.txt");
        std::fs::write(&path, format!("{}中文", "a".repeat(8191))).unwrap();
        assert!(matches!(
            resolve_plain_link(path.to_str().unwrap(), None),
            PlainLink::File(_)
        ));
        let mut contents = vec![b'a'; 9000];
        contents.push(0);
        std::fs::write(&path, &contents).unwrap();
        assert!(matches!(
            resolve_plain_link(path.to_str().unwrap(), None),
            PlainLink::Url(_)
        ));
        *contents.last_mut().unwrap() = 0xff;
        std::fs::write(&path, &contents).unwrap();
        assert!(matches!(
            resolve_plain_link(path.to_str().unwrap(), None),
            PlainLink::Url(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn terminal_file_special_paths_are_not_read_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("service.txt");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        assert_eq!(
            resolve_plain_link(socket.to_str().unwrap(), None),
            PlainLink::Ignore
        );
        let broken = dir.path().join("broken.txt");
        std::os::unix::fs::symlink(dir.path().join("absent"), &broken).unwrap();
        assert_eq!(
            resolve_plain_link(broken.to_str().unwrap(), None),
            PlainLink::Ignore
        );
        let folder = dir.path().join("folder.png");
        std::fs::create_dir(&folder).unwrap();
        assert!(matches!(
            resolve_plain_link(folder.to_str().unwrap(), None),
            PlainLink::Url(_)
        ));
    }

    #[test]
    fn terminal_file_document_urls_round_trip_special_characters() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("演示 #1 100%.pptx");
        std::fs::write(&path, b"PK archive fixture").unwrap();
        let PlainLink::Url(target) = resolve_plain_link(path.to_str().unwrap(), None) else {
            panic!("document must use the system opener");
        };
        let url = url::Url::parse(&target).unwrap();
        assert_eq!(url.scheme(), "file");
        assert_eq!(url.fragment(), None);
        assert_eq!(url.query(), None);
        assert!(target.contains("%23") && target.contains("%25") && target.contains("%20"));
        assert_eq!(url.to_file_path().unwrap(), path.canonicalize().unwrap());
    }

    #[test]
    fn extensionless_source_locations_are_not_url_schemes() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["README", "Makefile", "tel", "mailto"] {
            std::fs::write(dir.path().join(name), "text").unwrap();
        }
        for (raw, file, position) in [
            ("README:20", "README", (20, 1)),
            ("Makefile:12:3", "Makefile", (12, 3)),
        ] {
            assert_eq!(
                resolve_plain_link(raw, Some(dir.path())),
                PlainLink::File(TerminalFileLink {
                    path: dir.path().join(file).canonicalize().unwrap(),
                    position: Some(position),
                })
            );
        }
        for raw in ["tel:12345", "mailto:hello@example.com"] {
            assert_eq!(
                resolve_plain_link(raw, Some(dir.path())),
                PlainLink::Url(raw.into())
            );
        }
        assert_eq!(
            resolve_plain_link("MISSING:20", Some(dir.path())),
            PlainLink::Ignore
        );
    }

    #[test]
    fn plain_web_links_keep_ports_and_fragments() {
        for raw in [
            "https://example.com:8443/path:20",
            "https://example.com/#L20",
            "mailto:a@example.com",
            "tel:123456",
        ] {
            assert_eq!(resolve_plain_link(raw, None), PlainLink::Url(raw.into()));
        }
    }
}
