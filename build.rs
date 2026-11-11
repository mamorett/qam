fn main() {
    println!("cargo:rerun-if-env-changed=QAM_VERSION");
    println!("cargo:rerun-if-env-changed=CAMSNAP_VERSION");
    let version = std::env::var("QAM_VERSION")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::env::var("CAMSNAP_VERSION")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=QAM_BUILD_VERSION={version}");
}
