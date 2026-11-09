//! Updates the app itself from the GitHub releases: the latest tag against this build's version,
//! then that release's archive for this system unpacked over the program's folder, and a restart.
//! With the system's curl and tar (Windows 10+ ships both), no HTTP or archive crate.
//! (macOS: the release is the Swift app, which updates itself.)

use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "edsuuu/OpenHeadphones";
#[cfg(windows)]
const ASSET: &str = "jbl-headphones-windows-x64.zip";
#[cfg(all(not(windows), target_arch = "aarch64"))]
const ASSET: &str = "jbl-headphones-linux-arm64.tar.gz";
#[cfg(all(not(windows), not(target_arch = "aarch64")))]
const ASSET: &str = "jbl-headphones-linux-x64.tar.gz";

/// Updating, as the panel shows it.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Status {
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// A newer version, not installed (automatic updates off).
    Available(String),
    Installing(String),
    Failed,
}

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Whether `latest` (e.g. "0.4.0") is newer than this build.
pub fn newer(latest: &str) -> bool {
    let parse = |v: &str| v.split('.').map(|n| n.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    parse(latest) > parse(current())
}

fn command(program: &str) -> Command {
    #[allow(unused_mut)] // only Windows changes it
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: the app has no console to borrow
    }
    c
}

/// The latest release's version: /releases/latest redirects to /releases/tag/v<version>, or to
/// /releases while there is none (`Ok(None)`). `Err`: GitHub could not be reached.
pub fn latest() -> Result<Option<String>, ()> {
    // -L: a renamed repository (jbl-headphones → OpenHeadphones) redirects once more before the tag
    let out = command("curl").args(["-sIL", &format!("https://github.com/{REPO}/releases/latest")]).output().map_err(|_| ())?;
    version_in(&String::from_utf8_lossy(&out.stdout))
}

/// The version in the last redirect of these response headers.
fn version_in(headers: &str) -> Result<Option<String>, ()> {
    let location = headers.lines().filter(|l| l.to_ascii_lowercase().starts_with("location:")).last().ok_or(())?;
    let version = location.trim().rsplit_once("/tag/v").map(|(_, v)| v);
    Ok(version.filter(|v| v.starts_with(|c: char| c.is_ascii_digit())).map(str::to_string))
}

/// Whether this copy may replace itself: not a build in `target/` (it would turn into the release
/// and hide changes being made), nor one installed where the user cannot write (a .deb: apt updates
/// it).
pub fn can_install() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let Some(dir) = exe.parent() else { return false };
    if exe.components().any(|c| c.as_os_str() == "target") {
        return false;
    }
    let probe = dir.join(".jbl-update-probe");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(probe);
    ok
}

fn run(c: &mut Command) -> Result<(), String> {
    match c.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("{c:?}: {s}")),
        Err(e) => Err(format!("{c:?}: {e}")),
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &to.join(e.file_name()))?;
        } else {
            std::fs::copy(e.path(), to.join(e.file_name()))?;
        }
    }
    Ok(())
}

/// Downloads the latest release and puts it in place of this one (program, devices/, locales/).
/// The caller restarts. The archive holds them at its top level, as the release workflow packs it.
pub fn install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no folder")?.to_path_buf();
    let tmp = std::env::temp_dir().join("jbl-headphones-update");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let archive = tmp.join(ASSET);
    let url = format!("https://github.com/{REPO}/releases/latest/download/{ASSET}");
    run(command("curl").args(["-sfL", "-o"]).arg(&archive).arg(&url))?;
    run(command("tar").arg("-xf").arg(&archive).arg("-C").arg(&tmp))?;
    let new_exe = tmp.join(exe.file_name().ok_or("no name")?);
    if !new_exe.is_file() {
        return Err(format!("{} has no {}", ASSET, new_exe.display()));
    }
    // a running exe cannot be overwritten on Windows, but it can be renamed (removed next start)
    let old = exe.with_extension("old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::copy(&new_exe, &exe) {
        let _ = std::fs::rename(&old, &exe); // put the old one back: never leave no program
        return Err(e.to_string());
    }
    for data in ["devices", "locales"] {
        if tmp.join(data).is_dir() {
            let _ = std::fs::remove_dir_all(dir.join(data));
            copy_dir(&tmp.join(data), &dir.join(data)).map_err(|e| e.to_string())?;
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

/// Starts the program again (the new one, after `install`).
pub fn restart() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe).spawn();
    }
    std::process::exit(0);
}

/// Left behind by the last update.
pub fn clean_up() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("old"));
    }
}

fn auto_file() -> Option<PathBuf> {
    let base = match std::env::var_os(if cfg!(windows) { "APPDATA" } else { "XDG_CONFIG_HOME" }) {
        Some(b) => PathBuf::from(b),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("jbl-headphones").join("auto-update"))
}

/// Updates install by themselves when the app starts (on by default).
pub fn auto() -> bool {
    auto_file().and_then(|f| std::fs::read_to_string(f).ok()).is_none_or(|s| s.trim() != "off")
}

pub fn set_auto(on: bool) {
    if let Some(f) = auto_file() {
        let _ = f.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(f, if on { "on" } else { "off" });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(newer("99.0.0"));
        assert!(!newer(current()));
        assert!(!newer("0.0.1"));
        assert!(newer(&format!("{}.1", current()))); // 0.3.0.1 > 0.3.0
    }

    #[test]
    fn follows_the_renamed_repository() {
        let renamed = "HTTP/2 301\r\nlocation: https://github.com/edsuuu/OpenHeadphones/releases/latest\r\n\r\n\
                       HTTP/2 302\r\nlocation: https://github.com/edsuuu/OpenHeadphones/releases/tag/v0.4.0\r\n\r\nHTTP/2 200\r\n";
        assert_eq!(version_in(renamed), Ok(Some("0.4.0".into())));
        assert_eq!(version_in("HTTP/2 302\r\nLocation: https://github.com/x/y/releases\r\n"), Ok(None));
        assert_eq!(version_in(""), Err(()));
    }
}
