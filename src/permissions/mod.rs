//! macOS Full Disk Access check. macOS has no public preflight for it, so we
//! probe folders that only an app with that grant can read.

#[cfg(target_os = "macos")]
const FULL_DISK_ACCESS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

#[cfg(target_os = "macos")]
const PROBES: [&str; 3] = [
    "Library/Safari",
    "Library/Application Support/com.apple.TCC",
    "Library/Mail",
];

/// `Some(true)` granted, `Some(false)` denied, `None` unknown or not macOS.
pub fn full_disk_access() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        let home = std::path::PathBuf::from(std::env::var_os("HOME")?);
        PROBES
            .iter()
            .find_map(|path| probe(std::fs::read_dir(home.join(path)).map(|_| ())))
    }
    #[cfg(not(target_os = "macos"))]
    None
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn probe(result: std::io::Result<()>) -> Option<bool> {
    match result {
        Ok(()) => Some(true),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Some(false),
        Err(_) => None,
    }
}

/// Open the Full Disk Access pane in System Settings.
pub fn open_full_disk_access_settings() {
    #[cfg(target_os = "macos")]
    {
        if let Err(error) = std::process::Command::new("/usr/bin/open")
            .arg(FULL_DISK_ACCESS_URL)
            .spawn()
        {
            log::warn!("could not open System Settings: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::probe;
    use std::io::{Error, ErrorKind};

    #[test]
    fn denied_probe_means_not_granted_and_missing_probe_falls_through() {
        assert_eq!(probe(Ok(())), Some(true));
        assert_eq!(
            probe(Err(Error::from(ErrorKind::PermissionDenied))),
            Some(false)
        );
        assert_eq!(probe(Err(Error::from(ErrorKind::NotFound))), None);
    }
}
