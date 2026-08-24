fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // The debug CLI's generated clap command graph can exceed Windows' small
    // default main-thread stack. Release-LTO already fits, but development and
    // integration-test binaries must be reliable too.
    match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => println!("cargo:rustc-link-arg-bin=harness=/STACK:8388608"),
        _ => println!("cargo:rustc-link-arg-bin=harness=-Wl,--stack,8388608"),
    }
}
