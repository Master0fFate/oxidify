//! Where Oxidify keeps its files.
//!
//! Configuration, durable state (Spotify credentials), and disposable caches
//! (audio, artwork) live in the platform's conventional directories, so
//! clearing a cache never signs the user out and a config backup never
//! contains a credential.
//!
//! Oxidify began as a fork of Fastpotify, so a fresh install looks in the
//! directories Fastpotify used and imports the user's settings, sign-ins,
//! skins, and credentials once. New writes always go to Oxidify's own
//! directories; nothing is ever written back to the legacy ones.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

#[derive(Clone, Debug)]
pub struct AppDirs {
    pub config: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
}

impl AppDirs {
    pub fn discover() -> Self {
        let project = ProjectDirs::from("me", "master0ffate", "oxidify");
        match project {
            Some(project) => Self {
                config: project.config_dir().to_path_buf(),
                state: project
                    .state_dir()
                    .map(|path| path.to_path_buf())
                    .unwrap_or_else(|| project.data_local_dir().to_path_buf()),
                cache: project.cache_dir().to_path_buf(),
            },
            None => {
                let fallback = std::env::current_dir().unwrap_or_default();
                Self {
                    config: fallback.join("oxidify-config"),
                    state: fallback.join("oxidify-state"),
                    cache: fallback.join("oxidify-cache"),
                }
            }
        }
    }

    /// The directories Fastpotify, the project Oxidify is derived from, used
    /// on this machine. Read once at startup for the one-time import; never
    /// written to. Legacy compatibility only.
    pub fn legacy_fastpotify() -> Option<Self> {
        let project = ProjectDirs::from("me", "paolino", "fastpotify")?;
        Some(Self {
            config: project.config_dir().to_path_buf(),
            state: project
                .state_dir()
                .map(|path| path.to_path_buf())
                .unwrap_or_else(|| project.data_local_dir().to_path_buf()),
            cache: project.cache_dir().to_path_buf(),
        })
    }

    /// Copies the user's Fastpotify settings, sign-ins, skins, and playback
    /// credentials into these directories. Every file is copied only when it
    /// does not already exist here, so the import is safe to run at every
    /// start and never overwrites anything Oxidify has written. Caches and
    /// extracted helper binaries are left behind: they are disposable and
    /// large. Returns how many files were imported.
    pub fn import_legacy(&self, legacy: &AppDirs) -> usize {
        let mut imported = 0usize;
        fn copy_if_absent(from: &Path, to: &Path, imported: &mut usize) {
            if from.is_file() && !to.exists() {
                if let Some(parent) = to.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if std::fs::copy(from, to).is_ok() {
                    *imported += 1;
                }
            }
        }
        fn copy_tree_if_absent(from: &Path, to: &Path, imported: &mut usize) {
            if !from.is_dir() {
                return;
            }
            let mut stack = vec![from.to_path_buf()];
            while let Some(dir) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        stack.push(path);
                    } else {
                        let relative = path.strip_prefix(from).unwrap_or(&path);
                        copy_if_absent(&path, &to.join(relative), imported);
                    }
                }
            }
        }

        copy_if_absent(
            &legacy.settings_file(),
            &self.settings_file(),
            &mut imported,
        );
        copy_if_absent(&legacy.session_file(), &self.session_file(), &mut imported);
        copy_if_absent(
            &legacy.shared_web_token_file(),
            &self.shared_web_token_file(),
            &mut imported,
        );
        copy_if_absent(
            &legacy.personal_web_token_file(),
            &self.personal_web_token_file(),
            &mut imported,
        );
        copy_if_absent(
            &legacy.legacy_web_token_file(),
            &self.legacy_web_token_file(),
            &mut imported,
        );
        copy_tree_if_absent(&legacy.skins_dir(), &self.skins_dir(), &mut imported);
        copy_tree_if_absent(
            &legacy.credentials_dir(),
            &self.credentials_dir(),
            &mut imported,
        );
        copy_tree_if_absent(&legacy.volume_dir(), &self.volume_dir(), &mut imported);
        if imported > 0 {
            log::info!("imported {imported} file(s) from the previous Fastpotify install");
        }
        imported
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    /// Winamp skins the listener has added, as `.wsz` files or folders.
    pub fn skins_dir(&self) -> PathBuf {
        self.config.join("skins")
    }

    pub fn session_file(&self) -> PathBuf {
        self.state.join("session.json")
    }

    /// The plays counted on this computer, one line each, per account.
    pub fn history_file(&self, account_id: &str) -> PathBuf {
        self.state
            .join("history")
            .join(format!("{account_id}.jsonl"))
    }

    pub fn shared_web_token_file(&self) -> PathBuf {
        self.state.join("shared_web_api_token.json")
    }

    pub fn personal_web_token_file(&self) -> PathBuf {
        self.state.join("personal_web_api_token.json")
    }

    /// An older name for the shared Web API token, written before the app
    /// could hold two API sessions. Legacy compatibility only.
    pub fn legacy_web_token_file(&self) -> PathBuf {
        self.state.join("web_api_token.json")
    }

    /// The log of the current run, replaced at every start.
    pub fn log_file(&self) -> PathBuf {
        self.state.join("oxidify.log")
    }

    /// Where a panic is recorded before the process dies of it.
    pub fn panic_log(&self) -> PathBuf {
        self.state.join("panic.log")
    }

    pub fn credentials_dir(&self) -> PathBuf {
        self.state.join("credentials")
    }

    pub fn volume_dir(&self) -> PathBuf {
        self.state.join("volume")
    }

    pub fn audio_cache_dir(&self) -> PathBuf {
        self.cache.join("audio")
    }

    pub fn art_cache_dir(&self) -> PathBuf {
        self.cache.join("art")
    }

    pub fn lyrics_cache_dir(&self) -> PathBuf {
        self.cache.join("lyrics")
    }

    pub fn playlist_cache_dir(&self) -> PathBuf {
        self.cache.join("playlists")
    }

    /// Extracted helper binaries (bundled yt-dlp). Not cache; not config.
    pub fn bin_dir(&self) -> PathBuf {
        self.state.join("bin")
    }

    pub fn account_playlist_cache_dir(&self, account_id: &str) -> PathBuf {
        self.playlist_cache_dir().join(account_id)
    }

    /// The library as last seen, per account: the playlist list and Liked
    /// Songs, so they show the moment the app opens.
    pub fn account_library_cache_dir(&self, account_id: &str) -> PathBuf {
        self.cache.join("library").join(account_id)
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [&self.config, &self.state, &self.cache] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::AppDirs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oxidify-paths-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_fresh_install_imports_the_previous_settings_and_sign_ins() {
        let root = scratch("import");
        let legacy = AppDirs {
            config: root.join("legacy-config"),
            state: root.join("legacy-state"),
            cache: root.join("legacy-cache"),
        };
        std::fs::create_dir_all(legacy.state.join("credentials")).unwrap();
        std::fs::create_dir_all(legacy.config.join("skins")).unwrap();
        std::fs::write(legacy.settings_file(), "{}").unwrap();
        std::fs::write(legacy.session_file(), "{}").unwrap();
        std::fs::write(legacy.shared_web_token_file(), "{}").unwrap();
        std::fs::write(legacy.legacy_web_token_file(), "{}").unwrap();
        std::fs::write(legacy.credentials_dir().join("x.json"), "{}").unwrap();
        std::fs::write(legacy.skins_dir().join("skin.wsz"), "skin").unwrap();

        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        assert_eq!(dirs.import_legacy(&legacy), 6);
        assert!(dirs.settings_file().is_file());
        assert!(dirs.session_file().is_file());
        assert!(dirs.shared_web_token_file().is_file());
        assert!(dirs.legacy_web_token_file().is_file());
        assert!(dirs.credentials_dir().join("x.json").is_file());
        assert!(dirs.skins_dir().join("skin.wsz").is_file());

        // The import never overwrites: new values survive a second run.
        std::fs::write(dirs.settings_file(), r#"{"zoom": 2.0}"#).unwrap();
        assert_eq!(dirs.import_legacy(&legacy), 0);
        assert_eq!(
            std::fs::read_to_string(dirs.settings_file()).unwrap(),
            r#"{"zoom": 2.0}"#
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_install_without_a_legacy_needs_no_import() {
        let root = scratch("no-legacy");
        let legacy = AppDirs {
            config: root.join("legacy-config"),
            state: root.join("legacy-state"),
            cache: root.join("legacy-cache"),
        };
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        assert_eq!(dirs.import_legacy(&legacy), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_legacy_directories_are_fastpotifys() {
        // Legacy compatibility: these spell the directory names of the
        // Fastpotify install this app can import from, nothing more.
        let legacy = AppDirs::legacy_fastpotify().expect("platform directories");
        assert!(legacy.config.to_string_lossy().contains("fastpotify"));
        assert!(legacy.log_file().to_string_lossy().contains("fastpotify"));
    }
}
