//! `as` — import or switch a real Firefox profile.

use crate::error::Error;
use crate::launch::{launch_with_options, LaunchOptions};
use crate::profile_import::{import_profile, ImportReport};
use guise::StealthProfile;
use runtime_foxdriver::{Page, ProxyConfig};
use std::path::{Path, PathBuf};

/// Import `src` into `dest` (or a temp dir), then launch wearing it.
pub async fn as_profile(
    src: &Path,
    dest: Option<&Path>,
    profile: StealthProfile,
    headless: bool,
    proxy: Option<ProxyConfig>,
) -> Result<(Page, ImportReport), Error> {
    let dest_owned: PathBuf = match dest {
        Some(p) => p.to_path_buf(),
        None => {
            let dir = std::env::temp_dir().join(format!(
                "lurien-as-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            ));
            dir
        }
    };
    let report = import_profile(src, &dest_owned)?;
    let page = launch_with_options(LaunchOptions {
        profile,
        headless,
        profile_dir: Some(dest_owned.to_string_lossy().into_owned()),
        proxy,
    })
    .await?;
    Ok((page, report))
}
