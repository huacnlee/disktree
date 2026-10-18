use std::{path::PathBuf, process::ExitCode};

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;

#[cfg(not(feature = "development"))]
fn resources() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    #[cfg(target_os = "macos")]
    return Ok(exe.parent().unwrap().parent().unwrap().join("Resources"));
    #[cfg(target_os = "linux")]
    return Ok(exe.parent().unwrap().join("resources"));
}
fn app_source() -> Result<String, Box<dyn std::error::Error>> {
    #[cfg(feature = "development")]
    {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        Ok(nsh_bundler::bundle(&nsh_bundler::BundleOptions {
            entry: root.join("App.tsx"),
            minify: false,
            split: false,
            ui_sets: vec![disktree::__nsh_native_module()],
        })?
        .code)
    }
    #[cfg(not(feature = "development"))]
    {
        Ok(std::fs::read_to_string(resources()?.join("bundle.js"))?)
    }
}

fn app() -> Result<ExitCode, Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--version") {
        println!(
            "disktree {}",
            option_env!("DISKTREE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
        );
        return Ok(ExitCode::SUCCESS);
    }
    let source = app_source()?;
    platform::set_ui_sets(vec![disktree::__nsh_native_module()]);
    let mut options = platform::AppOptions {
        title: "Disktree".into(),
        size: (1280, 850),
        appearance: platform::Appearance::Dark,
        ..Default::default()
    };
    #[cfg(target_os = "linux")]
    {
        options.app_id = "com.huacnlee.disktree".into();
    }
    // The runtime's deterministic script interface is used by CI smoke tests.
    options.script = std::env::var("DISKTREE_UI_SCRIPT").ok();
    let result = platform::run_app(
        platform::AppSource::Bundle(platform::Bundle::source(source)),
        options,
    );
    #[cfg(target_os = "macos")]
    return Ok(result);
    #[cfg(target_os = "linux")]
    return Ok(if result == platform::gtk4::glib::ExitCode::SUCCESS {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    });
}
struct Logger;
impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn || metadata.target() == "js"
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("{} {}: {}", record.level(), record.target(), record.args());
        }
    }
    fn flush(&self) {}
}
static LOGGER: Logger = Logger;
fn main() -> ExitCode {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);
    match app() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("disktree: {error}");
            ExitCode::FAILURE
        }
    }
}
