//! Puts the app icon (assets/icon/krosshair.ico, drawn by tools/make-icon.ps1) into the exe as
//! resource 1: Explorer shows it for the file, gpui loads it for the window and the taskbar.
//! Needs rc.exe from the Windows SDK on PATH (the MSVC environment); without it the exe builds
//! with no icon and a warning.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/krosshair.rc");
    println!("cargo:rerun-if-changed=assets/icon/krosshair.ico");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    let res = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("krosshair.res");
    let built = Command::new("rc.exe")
        .args(["/nologo", "/fo"])
        .arg(&res)
        .arg("assets/icon/krosshair.rc")
        .status();
    match built {
        // link.exe takes a compiled .res like an object file.
        Ok(status) if status.success() => println!("cargo:rustc-link-arg-bins={}", res.display()),
        Ok(status) => panic!("rc.exe failed on assets/icon/krosshair.rc ({status})"),
        Err(e) => println!("cargo:warning=no app icon: can't run rc.exe ({e}); build in the MSVC environment"),
    }
}
