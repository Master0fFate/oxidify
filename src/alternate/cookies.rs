use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_BYTES: u64 = 1024 * 1024;
const HEADER: &str = "# Netscape HTTP Cookie File\n";
const HTTP_ONLY: &str = "#HttpOnly_";
const SCRATCH_DIR: &str = "youtube-cookies-scratch";

pub fn import(source: &Path, target: &Path) -> Result<usize, String> {
    let text = read_bounded(source)?;
    let (cookies, count) = youtube_cookies(&text)?;
    let temporary = scratch_file(target, "import").map_err(|error| keep_error(&error))?;
    crate::auth::write_private(&temporary, cookies.as_bytes())
        .and_then(|()| std::fs::rename(&temporary, target))
        .map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            keep_error(&error)
        })?;
    Ok(count)
}

pub fn remove(target: &Path) -> Result<(), String> {
    match std::fs::remove_file(target) {
        Err(error) if error.kind() != ErrorKind::NotFound => {
            Err(format!("Couldn't remove the YouTube cookies: {error}"))
        }
        _ => Ok(()),
    }
}

pub fn is_cookies_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"))
}

fn keep_error(error: &std::io::Error) -> String {
    format!("Couldn't keep the YouTube cookies: {error}")
}

fn read_bounded(source: &Path) -> Result<String, String> {
    let metadata = std::fs::metadata(source).map_err(|error| match error.kind() {
        ErrorKind::NotFound => "No file at that path.".to_string(),
        _ => format!("Couldn't read that file: {error}"),
    })?;
    if metadata.is_dir() {
        return Err("That is a folder, not a cookies.txt.".into());
    }
    if metadata.len() > MAX_BYTES {
        return Err("That file is too large for a cookies.txt.".into());
    }
    let mut text = String::new();
    std::fs::File::open(source)
        .and_then(|file| file.take(MAX_BYTES + 1).read_to_string(&mut text))
        .map_err(|error| match error.kind() {
            ErrorKind::InvalidData => "That file is not a text cookies.txt.".to_string(),
            ErrorKind::NotFound => "No file at that path.".to_string(),
            _ => format!("Couldn't read that file: {error}"),
        })?;
    if text.len() as u64 > MAX_BYTES {
        return Err("That file is too large for a cookies.txt.".into());
    }
    Ok(text)
}

fn youtube_cookies(text: &str) -> Result<(String, usize), String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.trim_start().starts_with(['[', '{']) {
        return Err("That is a JSON export. Export cookies.txt in Netscape format.".into());
    }
    let mut kept = String::from(HEADER);
    let count = text
        .lines()
        .filter(|line| push_youtube_entry(line, &mut kept))
        .count();
    if count == 0 {
        return Err("No youtube.com cookies in that file. Export them while signed in.".into());
    }
    Ok((kept, count))
}

fn push_youtube_entry(line: &str, out: &mut String) -> bool {
    let (prefix, entry) = match line.strip_prefix(HTTP_ONLY) {
        Some(entry) => (HTTP_ONLY, entry),
        None => ("", line),
    };
    if entry.starts_with('#') || entry.split('\t').count() != 7 {
        return false;
    }
    let Some((domain, rest)) = entry.split_once('\t') else {
        return false;
    };
    let Some((_, rest)) = rest.split_once('\t') else {
        return false;
    };
    let expires = rest.split('\t').nth(2).unwrap_or_default();
    if !is_youtube(domain) || !expires.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let subdomains = if domain.starts_with('.') {
        "TRUE"
    } else {
        "FALSE"
    };
    for part in [prefix, domain, "\t", subdomains, "\t", rest, "\n"] {
        out.push_str(part);
    }
    true
}

fn is_youtube(domain: &str) -> bool {
    const SUFFIX: &[u8] = b".youtube.com";
    let host = domain.trim_start_matches('.').as_bytes();
    host.eq_ignore_ascii_case(&SUFFIX[1..])
        || host.len() > SUFFIX.len()
            && host[host.len() - SUFFIX.len()..].eq_ignore_ascii_case(SUFFIX)
}

fn scratch_file(stored: &Path, kind: &str) -> std::io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = stored.with_file_name(SCRATCH_DIR);
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&dir)?;
    Ok(dir.join(format!(
        "{}-{}-{kind}.txt",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )))
}

pub(super) struct RunCookies(PathBuf);

impl RunCookies {
    pub(super) fn copy(stored: &Path) -> Option<Self> {
        let bytes = match std::fs::read(stored) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return None,
            Err(error) => {
                log::warn!("couldn't read the YouTube cookies for yt-dlp: {error}");
                return None;
            }
        };
        let path = match scratch_file(stored, "run") {
            Ok(path) => path,
            Err(error) => {
                log::warn!("couldn't hand the YouTube cookies to yt-dlp: {error}");
                return None;
            }
        };
        match crate::auth::write_private(&path, &bytes) {
            Ok(()) => Some(Self(path)),
            Err(error) => {
                log::warn!("couldn't hand the YouTube cookies to yt-dlp: {error}");
                let _ = std::fs::remove_file(&path);
                None
            }
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for RunCookies {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub(super) fn sweep_scratch(stored: &Path) {
    let Ok(entries) = std::fs::read_dir(stored.with_file_name(SCRATCH_DIR)) else {
        return;
    };
    let own = format!("{}-", std::process::id());
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_str()
            .is_none_or(|name| !name.starts_with(&own))
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
