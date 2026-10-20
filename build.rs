use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DISKTREE_VERSION");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    // Give direct `cargo run` / binary launches their own application identity,
    // too. AppKit can read this Mach-O section without an enclosing .app.
    let plist = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("Info.plist");
    fs::write(
        &plist,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.huacnlee.disktree</string>
<key>CFBundleName</key><string>Disktree</string>
<key>CFBundleDisplayName</key><string>Disktree</string>
<key>CFBundleExecutable</key><string>disktree</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#,
    )
    .expect("write embedded macOS application identity");
    println!(
        "cargo:rustc-link-arg-bin=disktree=-Wl,-sectcreate,__TEXT,__info_plist,{}",
        plist.display()
    );
}
