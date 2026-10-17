use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, ExitCode, Stdio},
};

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;

struct Scanner(Child);
impl Drop for Scanner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn resources() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    #[cfg(target_os = "macos")]
    return Ok(exe.parent().unwrap().parent().unwrap().join("Resources"));
    #[cfg(target_os = "linux")]
    return Ok(exe.parent().unwrap().join("resources"));
}
fn app() -> Result<ExitCode, Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--version") {
        println!(
            "disktree {}",
            option_env!("DISKTREE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
        );
        return Ok(ExitCode::SUCCESS);
    }
    let root = resources()?;
    let mut scanner = Scanner(
        Command::new(root.join("scanner/disktree-scanner"))
            .arg("--packaged")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let mut line = String::new();
    BufReader::new(scanner.0.stdout.take().unwrap()).read_line(&mut line)?;
    let connection: serde_json::Value = serde_json::from_str(&line)?;
    if connection
        .get("token")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .is_none()
    {
        return Err("scanner did not provide an authenticated connection".into());
    }
    let source = format!(
        "globalThis.__DISKTREE_CONNECTION__={};\n{}",
        serde_json::to_string(&connection)?,
        std::fs::read_to_string(root.join("bundle.js"))?
    );
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
    drop(scanner);
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
