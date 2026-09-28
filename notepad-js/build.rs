fn main() {
    napi_build::setup();
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let target_dir =
        std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| format!("{manifest_dir}/../target"));
    let bridge_dir = format!("{target_dir}/{profile}");
    println!("cargo:rustc-link-search=native={bridge_dir}");
    println!("cargo:rustc-link-lib=dylib=notepad_bridge");
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
}
