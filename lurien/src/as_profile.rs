//! `as` — import or switch a real Firefox profile.

use crate::error::Error;
use crate::launch::{launch_with_options, LaunchOptions, Launched};
use crate::profile_import::{import_profile, ImportReport};
use std::path::{Path, PathBuf};

/// Import `src` into `dest` (or a temp dir), then launch wearing it.
///
/// `opts` is the session's own launch contract: the imported profile replaces
/// [`LaunchOptions::profile_dir`] and nothing else, so permissions and the
/// position service survive the switch.
pub async fn as_profile(
    src: &Path,
    dest: Option<&Path>,
    opts: LaunchOptions,
) -> Result<(Launched, ImportReport), Error> {
    let dest_owned: PathBuf = match dest {
        Some(p) => p.to_path_buf(),
        None => std::env::temp_dir().join(format!(
            "lurien-as-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        )),
    };
    let report = import_profile(src, &dest_owned)?;
    let launched = launch_with_options(LaunchOptions {
        profile_dir: Some(dest_owned.to_string_lossy().into_owned()),
        ..opts
    })
    .await?;
    Ok((launched, report))
}
