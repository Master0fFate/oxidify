use std::ffi::{OsStr, OsString};
use std::os::windows::{ffi::OsStringExt, process::CommandExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::ptr::null_mut;

use anyhow::{Context, Result, ensure};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
use windows_sys::Win32::System::Threading::DETACHED_PROCESS;
use windows_sys::core::{PCWSTR, w};

use super::InstallTarget;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const UNINSTALL_KEY: PCWSTR = w!(
    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{7E9C4B61-3A28-4E75-9C0D-6F1B2A48D915}_is1"
);

fn uninstall_value(name: PCWSTR) -> Option<OsString> {
    // Windows paths cannot exceed 32,767 UTF-16 code units. One extra
    // element allows for the terminator that RegGetValueW supplies.
    let mut value = vec![0u16; 32_768];
    let mut bytes = size_of_val(value.as_slice()) as u32;
    // SAFETY: The key and value name are static NUL-terminated UTF-16
    // strings. The output buffer is writable for the supplied byte count.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            name,
            RRF_RT_REG_SZ,
            null_mut(),
            value.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let units = bytes as usize / size_of::<u16>();
    let value = value.get(..units)?.strip_suffix(&[0])?;
    (!value.contains(&0)).then(|| OsString::from_wide(value))
}

pub(super) fn detect(executable: &Path) -> Option<InstallTarget> {
    if uninstall_value(w!("DisplayVersion"))? != OsStr::new(CURRENT_VERSION) {
        return None;
    }
    let directory = PathBuf::from(uninstall_value(w!("Inno Setup: App Path"))?);
    if !directory.is_absolute() || directory.parent().is_none() {
        return None;
    }
    // A portable copy, including one with the same version, lives at a
    // different path. Keep the registered spelling for Inno's /DIR option.
    let installed_exe = directory.join("oxidify.exe").canonicalize().ok()?;
    (installed_exe == executable.canonicalize().ok()?).then_some(InstallTarget::Windows(directory))
}

pub(super) fn install(target: &Path, package: &Path, version: &str) -> Result<()> {
    ensure!(
        super::is_newer(version, CURRENT_VERSION)
            && super::is_newer(version, super::LAST_LEGACY_WINDOWS_RELEASE),
        "release does not support automatic Windows installation"
    );
    let executable = std::env::current_exe().context("cannot locate the running Oxidify")?;
    ensure!(
        matches!(detect(&executable), Some(InstallTarget::Windows(directory)) if directory == target),
        "the Oxidify installation changed after download"
    );
    let mut destination = OsString::from("/DIR=");
    destination.push(target);
    Command::new(package)
        .args([
            "/SP-",
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/NOCLOSEAPPLICATIONS",
            "/LOG",
        ])
        .arg(destination)
        .arg(format!("/OXIDIFYUPDATEPID={}", std::process::id()))
        .arg(concat!("/OXIDIFYFROMVERSION=", env!("CARGO_PKG_VERSION")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS)
        .spawn()
        .context("could not start the Oxidify installer")?;
    Ok(())
}
