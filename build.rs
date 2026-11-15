// Records the compiler version, target triple and build profile so that every
// result file says exactly which binary produced it (needed for comparability).
fn main() {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let ver = std::process::Command::new(rustc)
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=SYSCHECK_RUSTC={ver}");
    println!("cargo:rustc-env=SYSCHECK_TARGET={}", std::env::var("TARGET").unwrap_or_default());
    println!("cargo:rustc-env=SYSCHECK_PROFILE={}", std::env::var("PROFILE").unwrap_or_default());
    println!("cargo:rerun-if-changed=build.rs");
}
