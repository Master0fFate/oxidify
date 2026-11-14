use std::ffi::CString;
use std::fs::{self, Metadata};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use tempfile::{Builder, TempDir};

use super::InstallTarget;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;

pub(super) fn detect(executable: &Path) -> Option<InstallTarget> {
    platform::detect(executable)
}

pub(super) fn prepare(
    target: &InstallTarget,
    package: &Path,
    version: &str,
) -> Result<PlatformUpdate> {
    platform::prepare(target, package, version)
}

pub(super) struct PlatformUpdate {
    stage: TempDir,
    destination: PathBuf,
    expected: FileIdentity,
}

impl PlatformUpdate {
    pub(super) fn install(self) -> Result<()> {
        platform::install(self)
    }
}

#[derive(PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
    changed: (i64, i64),
}

impl FileIdentity {
    fn read(path: &Path) -> Result<Self> {
        let metadata = fs::symlink_metadata(path).context("could not inspect installed Oxidify")?;
        ensure!(
            !metadata.is_symlink(),
            "Oxidify changed after the update was selected"
        );
        Ok(Self::from_metadata(&metadata))
    }

    fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }

    fn require_unchanged(&self, path: &Path) -> Result<()> {
        ensure!(
            &Self::read(path)? == self,
            "Oxidify changed after the update was prepared"
        );
        Ok(())
    }
}

fn owned_by_user(metadata: &Metadata) -> bool {
    // SAFETY: getuid/geteuid take no arguments and have no preconditions.
    let (real, effective) = unsafe { (libc::getuid(), libc::geteuid()) };
    real == effective && effective != 0 && metadata.uid() == effective
}

fn can_replace_in(parent: &Path) -> bool {
    CString::new(parent.as_os_str().as_bytes()).is_ok_and(|path| {
        // SAFETY: path is a live NUL-terminated string. No elevated process
        // reaches this check because the installed item must be user-owned.
        unsafe { libc::access(path.as_ptr(), libc::W_OK | libc::X_OK) == 0 }
    })
}

fn sibling_stage(destination: &Path) -> Result<TempDir> {
    let parent = destination
        .parent()
        .context("installed Oxidify has no parent directory")?;
    Builder::new()
        .prefix(".oxidify-update-")
        .tempdir_in(parent)
        .context("could not create a staging directory next to Oxidify")
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::OsStr;
    use std::fs::{File, Permissions};
    use std::io::{self, Read, Seek};
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};

    use super::*;

    const STAGED_BINARY: &str = "oxidify";
    const MAX_BINARY_BYTES: u64 = 256 * 1024 * 1024;
    const REQUIRED_FILES: [&str; 4] = [
        "README.md",
        "LICENSE",
        "NOTICE",
        "packaging/applications/oxidify.desktop",
    ];

    fn release_target() -> Option<(&'static str, u16)> {
        match std::env::consts::ARCH {
            "x86_64" if cfg!(target_env = "gnu") => Some(("x86_64-unknown-linux-gnu", 62)),
            "aarch64" if cfg!(target_env = "gnu") => Some(("aarch64-unknown-linux-gnu", 183)),
            _ => None,
        }
    }

    pub(super) fn detect(executable: &Path) -> Option<InstallTarget> {
        let (target, _) = release_target()?;
        let home = directories::UserDirs::new()?
            .home_dir()
            .canonicalize()
            .ok()?;
        let folder = executable.parent()?;
        let original_version = folder
            .file_name()?
            .to_str()?
            .strip_prefix("oxidify-v")?
            .strip_suffix(&format!("-{target}"))?;
        if !(original_version == CURRENT_VERSION
            || super::super::is_newer(CURRENT_VERSION, original_version))
            || !folder.starts_with(home)
            || !owned_by_user(&folder.metadata().ok()?)
            || !can_replace_in(folder)
            || executable.file_name() != Some(OsStr::new(STAGED_BINARY))
            || executable.canonicalize().ok()? != executable
            || !owned_regular_executable(executable)
            || !REQUIRED_FILES
                .iter()
                .all(|name| folder.join(name).is_file())
            || tar_path().is_none()
        {
            return None;
        }
        Some(InstallTarget::Linux(executable.to_path_buf()))
    }

    fn owned_regular_executable(path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok_and(|metadata| {
            metadata.is_file()
                && owned_by_user(&metadata)
                && metadata.mode() & 0o7000 == 0
                && metadata.mode() & 0o100 != 0
        })
    }

    pub(super) fn prepare(
        target: &InstallTarget,
        package: &Path,
        version: &str,
    ) -> Result<PlatformUpdate> {
        let InstallTarget::Linux(binary) = target;
        let (triple, machine) = release_target().context("unsupported Linux architecture")?;
        ensure!(
            detect(binary).as_ref() == Some(target),
            "the Oxidify installation changed"
        );
        let expected = FileIdentity::read(binary)?;
        ensure!(
            expected == FileIdentity::from_metadata(&fs::metadata("/proc/self/exe")?),
            "the running Oxidify binary moved or was replaced"
        );
        let stage = sibling_stage(binary)?;
        let path = stage.path().join(STAGED_BINARY);
        let member = format!("oxidify-v{version}-{triple}/{STAGED_BINARY}");
        extract_binary(package, &member, &path, machine, binary.metadata()?.mode())?;
        Ok(PlatformUpdate {
            stage,
            destination: binary.clone(),
            expected,
        })
    }

    fn tar_path() -> Option<&'static str> {
        ["/usr/bin/tar", "/bin/tar"]
            .into_iter()
            .find(|path| Path::new(path).is_file())
    }

    fn tar(package: &Path, member: &str, operation: &str) -> Result<Command> {
        let mut command =
            Command::new(tar_path().context("could not find a system tar executable")?);
        command
            .args([operation, "--no-recursion", "--file"])
            .arg(package)
            .args(["--", member])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .stdout(Stdio::piped());
        Ok(command)
    }

    fn read_tar(command: &mut Command, output: &mut impl io::Write, limit: u64) -> Result<u64> {
        let mut child = command.spawn().context("could not start tar")?;
        let stdout = child.stdout.take().expect("stdout was set to piped");
        let copied = io::copy(&mut stdout.take(limit + 1), output);
        if !matches!(copied, Ok(bytes) if bytes <= limit) {
            // A failed write or an oversized stream must not leave a child behind.
            let _ = child.kill();
        }
        let status = child.wait().context("could not wait for tar")?;
        let bytes = copied.context("could not read the update archive")?;
        ensure!(
            bytes <= limit,
            "update archive member exceeds its size limit"
        );
        ensure!(
            status.success(),
            "could not read the Oxidify member from the update archive"
        );
        Ok(bytes)
    }

    fn extract_binary(
        package: &Path,
        member: &str,
        path: &Path,
        machine: u16,
        mode: u32,
    ) -> Result<()> {
        // Check there is exactly one regular member with this path. Then stream
        // just that member into our own private file instead of extracting paths.
        let mut listing = Vec::new();
        read_tar(&mut tar(package, member, "-tzv")?, &mut listing, 1024)?;
        let mut lines = std::str::from_utf8(&listing)?.lines();
        ensure!(
            lines
                .next()
                .is_some_and(|line| line.starts_with('-') && line.ends_with(&format!(" {member}")))
                && lines.next().is_none(),
            "expected a single regular Oxidify binary in the update archive"
        );
        let mut output = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        read_tar(
            &mut tar(package, member, "-xzO")?,
            &mut output,
            MAX_BINARY_BYTES,
        )?;
        output.rewind()?;
        let mut header = [0; 20];
        output.read_exact(&mut header)?;
        ensure!(
            &header[..6] == b"\x7fELF\x02\x01"
                && u16::from_le_bytes([header[18], header[19]]) == machine,
            "update executable has the wrong Linux architecture"
        );
        output.set_permissions(Permissions::from_mode(mode & 0o777))?;
        output
            .sync_all()
            .context("could not save the staged binary")
    }

    pub(super) fn install(update: PlatformUpdate) -> Result<()> {
        ensure!(
            detect(&update.destination).is_some(),
            "the Oxidify installation changed"
        );
        replace_binary(
            &update.stage.path().join(STAGED_BINARY),
            &update.destination,
            &update.expected,
        )
    }

    fn replace_binary(new: &Path, destination: &Path, expected: &FileIdentity) -> Result<()> {
        expected.require_unchanged(destination)?;
        fs::rename(new, destination).context("could not replace the Oxidify binary")
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn linux_replaces_only_the_unchanged_destination() {
            let dir = tempfile::tempdir().unwrap();
            let binary = dir.path().join("oxidify");
            let staged = dir.path().join("staged");
            let manual = dir.path().join("manual");
            fs::write(&binary, b"old").unwrap();
            fs::write(&staged, b"new").unwrap();
            fs::write(&manual, b"newer").unwrap();
            let expected = FileIdentity::read(&binary).unwrap();
            let mut running = File::open(&binary).unwrap();
            replace_binary(&staged, &binary, &expected).unwrap();
            assert_eq!(fs::read(&binary).unwrap(), b"new");
            let mut previous = Vec::new();
            running.read_to_end(&mut previous).unwrap();
            assert_eq!(previous, b"old");

            let expected = FileIdentity::read(&binary).unwrap();
            fs::write(&staged, b"intermediate").unwrap();
            fs::rename(&manual, &binary).unwrap();
            assert!(replace_binary(&staged, &binary, &expected).is_err());
            assert_eq!(fs::read(&binary).unwrap(), b"newer");
        }

        #[test]
        fn linux_extracts_only_the_expected_regular_binary() {
            let dir = tempfile::tempdir().unwrap();
            let release = dir.path().join("release");
            fs::create_dir(&release).unwrap();
            let binary = release.join("oxidify");
            let mut header = [0; 20];
            header[..6].copy_from_slice(b"\x7fELF\x02\x01");
            header[18..].copy_from_slice(&62u16.to_le_bytes());
            fs::write(&binary, header).unwrap();
            std::os::unix::fs::symlink("oxidify", release.join("link")).unwrap();
            let package = dir.path().join("update.tar.gz");
            assert!(
                Command::new(tar_path().unwrap())
                    .args(["-czf"])
                    .arg(&package)
                    .arg("release")
                    .current_dir(dir.path())
                    .status()
                    .unwrap()
                    .success()
            );
            let staged = dir.path().join("staged");
            extract_binary(&package, "release/oxidify", &staged, 62, 0o700).unwrap();
            assert_eq!(staged.metadata().unwrap().mode() & 0o777, 0o700);
            assert_eq!(fs::read(staged).unwrap(), header);
            assert!(
                extract_binary(
                    &package,
                    "release/oxidify",
                    &dir.path().join("wrong-arch"),
                    183,
                    0o755
                )
                .is_err()
            );
            assert!(
                extract_binary(
                    &package,
                    "release/link",
                    &dir.path().join("link"),
                    62,
                    0o755
                )
                .is_err()
            );
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::io;
    use std::process::{Command, Stdio};

    use serde::Deserialize;

    use super::*;

    const APP: &str = "Oxidify.app";
    const EXECUTABLE: &str = "Contents/MacOS/oxidify";
    const BUNDLE_ID: &str = "me.master0ffate.oxidify";
    const HDIUTIL: &str = "/usr/bin/hdiutil";

    pub(super) fn detect(executable: &Path) -> Option<InstallTarget> {
        let bundle = executable.ancestors().nth(3)?;
        let parent = bundle.parent()?;
        let home_apps = directories::UserDirs::new()?
            .home_dir()
            .canonicalize()
            .ok()?
            .join("Applications");
        if executable != bundle.join(EXECUTABLE)
            || bundle.file_name()?.to_str()? != APP
            || (parent != Path::new("/Applications") && parent != home_apps)
            || executable.canonicalize().ok()? != executable
            || !can_replace_in(parent)
            || !owned_by_user(&bundle.metadata().ok()?)
            || !owned_by_user(&executable.metadata().ok()?)
            || bundle.join("Contents/_MASReceipt").exists()
            || homebrew_managed()
        {
            return None;
        }
        check_bundle(bundle, CURRENT_VERSION).ok()?;
        Some(InstallTarget::Macos(bundle.to_path_buf()))
    }

    fn homebrew_managed() -> bool {
        fn has_receipt(caskroom: &Path) -> bool {
            ["oxidify", "oxidify-app"]
                .iter()
                .any(|name| fs::symlink_metadata(caskroom.join(name)).is_ok())
        }

        // Casks can move a bundle to Applications, so its final path is not
        // enough to distinguish a direct install from Homebrew's copy.
        let default = ["/opt/homebrew/Caskroom", "/usr/local/Caskroom"];
        if default.iter().any(|path| has_receipt(Path::new(path)))
            || std::env::var_os("HOMEBREW_CASKROOM")
                .is_some_and(|path| has_receipt(Path::new(&path)))
            || std::env::var_os("HOMEBREW_PREFIX")
                .is_some_and(|prefix| has_receipt(&Path::new(&prefix).join("Caskroom")))
        {
            return true;
        }
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths).any(|bin| {
                bin.is_absolute()
                    && bin.join("brew").is_file()
                    && bin
                        .parent()
                        .is_some_and(|prefix| has_receipt(&prefix.join("Caskroom")))
            })
        })
    }

    #[derive(Deserialize)]
    struct BundleInfo {
        #[serde(rename = "CFBundleIdentifier")]
        identifier: String,
        #[serde(rename = "CFBundleExecutable")]
        executable: String,
        #[serde(rename = "CFBundleShortVersionString")]
        version: String,
        #[serde(rename = "CFBundleVersion")]
        build_version: String,
    }

    fn check_bundle(bundle: &Path, version: &str) -> Result<()> {
        let plist = bundle.join("Contents/Info.plist");
        let metadata = fs::symlink_metadata(&plist).context("could not read Oxidify Info.plist")?;
        let binary = bundle.join(EXECUTABLE);
        ensure!(
            metadata.is_file()
                && metadata.len() <= 64 * 1024
                && fs::symlink_metadata(binary)?.is_file(),
            "invalid Oxidify bundle layout"
        );
        let output = tool("/usr/bin/plutil")
            .args(["-convert", "json", "-o", "-", "--"])
            .arg(plist)
            .stdout(Stdio::piped())
            .output()
            .context("could not run plutil")?;
        ensure!(
            output.status.success(),
            "could not read Oxidify bundle metadata"
        );
        let info: BundleInfo =
            serde_json::from_slice(&output.stdout).context("invalid Oxidify bundle metadata")?;
        ensure!(
            info.identifier == BUNDLE_ID
                && info.executable == "oxidify"
                && info.version == version
                && info.build_version == version,
            "the app bundle identity or version does not match Oxidify {version}"
        );
        Ok(())
    }

    pub(super) fn prepare(
        target: &InstallTarget,
        package: &Path,
        version: &str,
    ) -> Result<PlatformUpdate> {
        let InstallTarget::Macos(bundle) = target;
        let binary = bundle.join(EXECUTABLE);
        ensure!(
            detect(&binary).as_ref() == Some(target),
            "the Oxidify installation changed"
        );
        let expected = FileIdentity::read(&binary)?;
        let stage = sibling_stage(bundle)?;
        stage_bundle(package, version, &stage.path().join(APP))?;
        Ok(PlatformUpdate {
            stage,
            destination: bundle.clone(),
            expected,
        })
    }

    fn tool(executable: &str) -> Command {
        let mut command = Command::new(executable);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    fn run(command: &mut Command, description: &str) -> Result<()> {
        let status = command.status().with_context(|| description.to_owned())?;
        ensure!(status.success(), "{description} ({status})");
        Ok(())
    }

    fn stage_bundle(package: &Path, version: &str, staged: &Path) -> Result<()> {
        let mount = Builder::new().prefix("oxidify-dmg-").tempdir_in("/tmp")?;
        let mountpoint = mount.path().join("image");
        fs::create_dir(&mountpoint)?;
        let mounted = run(
            tool(HDIUTIL)
                .args([
                    "attach",
                    "-readonly",
                    "-nobrowse",
                    "-noautoopen",
                    "-mountpoint",
                ])
                .arg(&mountpoint)
                .arg(package),
            "could not mount the Oxidify disk image",
        );
        if let Err(error) = mounted {
            // A failed attach can leave the mountpoint in use.
            let _ = fs::remove_dir(&mountpoint);
            if mountpoint.exists() {
                let _ = mount.keep();
            }
            return Err(error);
        }
        let result = (|| {
            let source = mountpoint.join(APP);
            ensure!(
                fs::symlink_metadata(&source)?.is_dir(),
                "Oxidify.app is missing from the disk image"
            );
            check_bundle(&source, version)?;
            run(
                tool("/usr/bin/ditto").arg(source).arg(staged),
                "could not copy the new app bundle",
            )?;
            run(
                tool("/usr/bin/codesign")
                    .args(["--verify", "--deep", "--strict"])
                    .arg(staged),
                "the new Oxidify bundle did not pass code signature verification",
            )
        })();
        if let Err(error) = run(
            tool(HDIUTIL).arg("detach").arg(&mountpoint),
            "could not detach the Oxidify disk image",
        ) {
            // Don't recursively delete through a mounted image.
            let _ = mount.keep();
            return Err(error);
        }
        result
    }

    pub(super) fn install(update: PlatformUpdate) -> Result<()> {
        let binary = update.destination.join(EXECUTABLE);
        ensure!(
            detect(&binary).is_some(),
            "the Oxidify installation changed"
        );
        update.expected.require_unchanged(&binary)?;
        // After a successful swap, the staging directory contains the old
        // bundle and removes it when dropped.
        swap_bundles(&update.stage.path().join(APP), &update.destination)
    }

    fn swap_bundles(staged: &Path, destination: &Path) -> Result<()> {
        let staged = CString::new(staged.as_os_str().as_bytes())?;
        let destination = CString::new(destination.as_os_str().as_bytes())?;
        // SAFETY: both arguments are live NUL-terminated strings. Swapping
        // whole bundles keeps a working app at the destination even on error.
        let swapped =
            unsafe { libc::renamex_np(staged.as_ptr(), destination.as_ptr(), libc::RENAME_SWAP) };
        if swapped != 0 {
            return Err(io::Error::last_os_error())
                .context("could not replace the Oxidify app bundle");
        }
        Ok(())
    }

    #[cfg(test)]
    #[test]
    fn macos_swaps_complete_app_bundles() {
        let dir = tempfile::tempdir().unwrap();
        let before = dir.path().join("before");
        let after = dir.path().join("after");
        fs::create_dir(&before).unwrap();
        fs::create_dir(&after).unwrap();
        fs::write(before.join("old"), b"old").unwrap();
        fs::write(after.join("new"), b"new").unwrap();
        swap_bundles(&after, &before).unwrap();
        assert_eq!(fs::read(before.join("new")).unwrap(), b"new");
        assert_eq!(fs::read(after.join("old")).unwrap(), b"old");
    }
}
