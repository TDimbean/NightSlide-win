use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/white_crescent_moon.ico");

    let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86") => "x86",
        Ok("aarch64") => "arm64",
        _ => "x64",
    };
    let mut candidates = Vec::new();
    if let Some(rc) = env::var_os("RC") {
        candidates.push(PathBuf::from(rc));
    }
    candidates.push(PathBuf::from("rc.exe"));
    if let Some(program_files) = env::var_os("ProgramFiles(x86)") {
        let bin = PathBuf::from(program_files).join("Windows Kits/10/bin");
        candidates.push(bin.join(arch).join("rc.exe"));
        if let Ok(versions) = bin.read_dir() {
            for version in versions.flatten() {
                candidates.push(version.path().join(arch).join("rc.exe"));
            }
        }
    }
    let rc = candidates
        .into_iter()
        .find(|candidate| Command::new(candidate).arg("/?").output().is_ok())
        .expect("Windows SDK rc.exe is required to embed the app icon");
    let resource = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"))
        .join("nightslide.res");
    let output = Command::new(rc)
        .arg("/nologo")
        .arg(format!("/fo{}", resource.display()))
        .arg("app.rc")
        .output()
        .expect("failed to run rc.exe");
    if !output.status.success() {
        panic!("rc.exe failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    println!("cargo:rustc-link-arg={}", resource.display());
}
