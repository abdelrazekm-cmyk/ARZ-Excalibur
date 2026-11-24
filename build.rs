use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/app_icon.ico");
    println!("cargo:rerun-if-changed=assets/app_icon.rc");

    if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let res_file = out_dir.join("app_icon.res");

    let rc_candidates = [
        r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\rc.exe",
        "rc.exe",
    ];

    for rc in rc_candidates {
        if let Ok(status) = Command::new(rc)
            .args(["/nologo", "/fo"])
            .arg(&res_file)
            .arg("assets/app_icon.rc")
            .status()
        {
            if status.success() && res_file.exists() {
                println!("cargo:rustc-link-arg-bins={}", res_file.display());
                break;
            }
        }
    }
}
