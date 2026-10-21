use std::process::ExitCode;

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;

fn app() -> Result<ExitCode, Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--version") {
        println!(
            "disktree {}",
            option_env!("DISKTREE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
        );
        return Ok(ExitCode::SUCCESS);
    }
    platform::set_ui_sets(vec![disktree::__nsh_native_module()]);
    let mut options = platform::AppOptions {
        app_id: "com.huacnlee.disktree".into(),
        title: "Disktree".into(),
        size: (1280, 850),
        appearance: platform::Appearance::Dark,
        ..Default::default()
    };
    let performance = std::env::var_os("DISKTREE_FPS").is_some();
    options.trace = performance;
    #[cfg(target_os = "macos")]
    {
        options.fps = performance;
    }
    let result = platform::run_app(
        platform::AppSource::Rust(
            platform::Root::of::<disktree::App>().app_id("com.huacnlee.disktree"),
        ),
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
        metadata.level() <= log::Level::Warn
            || (metadata.target() == "nsh::perf" && std::env::var_os("DISKTREE_FPS").is_some())
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
    #[cfg(target_os = "macos")]
    if std::env::var_os("__CFBundleIdentifier").is_some_and(|id| id != "com.huacnlee.disktree") {
        use std::os::unix::process::CommandExt;
        // CoreFoundation can cache the launching terminal's bundle identity
        // before main. Re-exec with a clean environment rather than changing
        // it after AppKit has initialized. exec preserves the PID and arguments.
        let error = match std::env::current_exe() {
            Ok(exe) => std::process::Command::new(exe)
                .args(std::env::args_os().skip(1))
                .env_remove("__CFBundleIdentifier")
                .exec(),
            Err(error) => error,
        };
        eprintln!("disktree: could not reset macOS application identity: {error}");
        return ExitCode::FAILURE;
    }
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
