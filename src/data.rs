//! Runtime data (`devices/`, `locales/`): the first folder that has it, from the program's
//! folder upward — next to the exe in a release, the project root in a dev build.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub fn root() -> Option<&'static Path> {
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ROOT.get_or_init(|| {
        let exe = std::env::current_exe().ok()?;
        exe.ancestors().skip(1).find(|d| d.join("devices").is_dir()).map(Path::to_path_buf)
    })
    .as_deref()
}
