fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let version = std::env::var_os("RUSTC")
        .and_then(|rustc| {
            std::process::Command::new(rustc)
                .arg("--version")
                .output()
                .ok()
        })
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_else(|| "Unavailable".into());
    println!("cargo:rustc-env=SYSTEM_RUSTC={}", version.trim());
    println!(
        "cargo:rustc-env=SYSTEM_PROFILE={}",
        std::env::var("PROFILE").unwrap_or_else(|_| "Unavailable".into())
    );
}
