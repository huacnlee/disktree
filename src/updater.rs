//! App-owned update flow over NSH's manifest, download and installation rules.
use nsh_core::prelude::*;
use nsh_updater::{Available, Config, Download, install::Installation};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

type Result<T> = std::result::Result<T, String>;

pub(crate) struct Update {
    pub available: Available,
    installation: Installation,
    can_install: bool,
}

pub(crate) struct Downloaded {
    update: Arc<Update>,
    path: PathBuf,
}

impl Drop for Downloaded {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn config_path(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let resources = if cfg!(target_os = "macos") {
        dir.parent()?.join("Resources/nsh")
    } else {
        dir.join("nsh")
    };
    Some(resources.join("assets").join(nsh_updater::CONFIG_ASSET))
}

fn get_small(url: &str) -> Result<Vec<u8>> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .new_agent();
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1_048_576 {
        return Err("The update manifest is too large.".into());
    }
    Ok(bytes)
}

impl Update {
    pub fn check() -> Result<Option<Self>> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let path = config_path(&exe).ok_or("Cannot locate the installed app.")?;
        let bytes = fs::read(path)
            .map_err(|_| "Updates are available in installed builds of Disktree.".to_owned())?;
        let config = Config::parse(&bytes).map_err(|e| e.to_string())?;
        let installation = Installation::current(&config.id, &bytes)
            .ok_or("This installation cannot update itself.")?;
        let manifest = get_small(&config.manifest_url())?;
        let signature = config
            .signature_url()
            .map(|url| get_small(&url))
            .transpose()?;
        let Some(available) = config
            .check(&manifest, signature.as_deref(), &nsh_updater::platform())
            .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        let can_install = available.sha256.is_some() && installation.can_install();
        installation.remove_leftovers();
        Ok(Some(Self {
            available,
            installation,
            can_install,
        }))
    }

    pub fn can_install(&self) -> bool {
        self.can_install
    }

    pub fn download(
        self: Arc<Self>,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<Downloaded> {
        if !self.can_install() {
            return Err(
                "Download the update from GitHub Releases to replace this installation.".into(),
            );
        }
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(600)))
            .build()
            .new_agent();
        let mut response = agent
            .get(&self.available.url)
            .call()
            .map_err(|e| e.to_string())?;
        // Keep the archive outside the directory that will be replaced. A
        // unique, exclusively created file also prevents concurrent downloads
        // from truncating a previously verified archive.
        let dir = crate::scanner::state_file()
            .parent()
            .ok_or("No update directory.")?
            .join("updates");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!(
            "download-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let downloaded = Downloaded {
            update: self.clone(),
            path,
        };
        let total = self.available.size.or_else(|| {
            response
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
        });
        let mut check = Download::new(
            self.available.sha256.as_deref().unwrap(),
            self.available.size,
        );
        let mut reader = response.body_mut().as_reader();
        let mut chunk = [0_u8; 32768];
        loop {
            let count = reader.read(&mut chunk).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            let received = check.update(&chunk[..count]).map_err(|e| e.to_string())?;
            file.write_all(&chunk[..count]).map_err(|e| e.to_string())?;
            progress(received, total);
        }
        check.finish().map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        Ok(downloaded)
    }
}

impl Downloaded {
    pub fn install_and_launch(&self) -> Result<()> {
        // Recheck the file immediately before extraction, including downloads
        // kept while the user decides when to restart.
        let available = &self.update.available;
        let mut check = Download::new(available.sha256.as_deref().unwrap(), available.size);
        let mut file = fs::File::open(&self.path).map_err(|e| e.to_string())?;
        let mut chunk = [0_u8; 32768];
        loop {
            let count = file.read(&mut chunk).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            check.update(&chunk[..count]).map_err(|e| e.to_string())?;
        }
        check.finish().map_err(|e| e.to_string())?;
        self.update
            .installation
            .install_and_launch(&self.path)
            .map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&self.path);
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Controls {
    pub open: Signal<bool>,
    pub busy: Signal<bool>,
    pub message: Signal<String>,
    pub update: Signal<Option<Arc<Update>>>,
    pub downloaded: Signal<Option<Arc<Downloaded>>>,
    owner: Owner,
}

impl Controls {
    pub fn new() -> Self {
        Self {
            open: signal(false),
            busy: signal(false),
            message: signal(String::new()),
            update: signal_with(None, SignalEquals::Never),
            downloaded: signal_with(None, SignalEquals::Never),
            owner: Owner::current(),
        }
    }

    pub fn check(self) {
        if self.busy.get_untracked() {
            return;
        }
        self.open.set(true);
        self.busy.set(true);
        self.update.set(None);
        self.downloaded.set(None);
        self.message.set("Checking for updates…".into());
        self.owner.run(move || {
            spawn(async move {
                match blocking(Update::check).await {
                    Ok(Ok(Some(update))) => {
                        self.message.set(format!(
                            "Disktree {} is available.",
                            update.available.version
                        ));
                        self.update.set(Some(Arc::new(update)));
                    }
                    Ok(Ok(None)) => self.message.set("Disktree is up to date.".into()),
                    Ok(Err(error)) => self.message.set(error),
                    Err(error) => self.message.set(error.to_string()),
                }
                self.busy.set(false);
            });
        });
    }

    pub fn download(self) {
        if self.busy.get_untracked() {
            return;
        }
        let Some(update) = self.update.get_untracked() else {
            return;
        };
        self.busy.set(true);
        self.message.set("Downloading update…".into());
        self.owner.run(move || {
            spawn(async move {
                let progress = Arc::new(Mutex::new((0_u64, None)));
                let received = progress.clone();
                let task = blocking(move || {
                    update.download(|bytes, total| *received.lock().unwrap() = (bytes, total))
                });
                while !task.is_finished() {
                    let (bytes, total) = *progress.lock().unwrap();
                    if let Some(total) = total.filter(|&total| total > 0) {
                        self.message.set(format!(
                            "Downloading update… {}%",
                            bytes.saturating_mul(100) / total
                        ));
                    }
                    timer(Duration::from_millis(100)).await;
                }
                match task.await {
                    Ok(Ok(downloaded)) => {
                        self.downloaded.set(Some(Arc::new(downloaded)));
                        self.message
                            .set("Update downloaded. Restart Disktree to install it.".into());
                    }
                    Ok(Err(error)) => self.message.set(error),
                    Err(error) => self.message.set(error.to_string()),
                }
                self.busy.set(false);
            });
        });
    }

    pub fn install(self) {
        if self.busy.get_untracked() {
            return;
        }
        let Some(downloaded) = self.downloaded.get_untracked() else {
            return;
        };
        self.busy.set(true);
        self.message.set("Installing update…".into());
        self.owner.run(move || {
            spawn(async move {
                let error = match blocking(move || downloaded.install_and_launch()).await {
                    Ok(Err(error)) => error,
                    Err(error) => error.to_string(),
                    Ok(Ok(())) => {
                        // Let the native window and renderer shut down on
                        // their own thread before main returns.
                        nsh_core::element::desktop::close_window();
                        return;
                    }
                };
                self.downloaded.set(None);
                self.message.set(error);
                self.busy.set(false);
            });
        });
    }
}
