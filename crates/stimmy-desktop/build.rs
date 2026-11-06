//! libmpv is opened at runtime, and refers to its own dependencies through
//! @rpath. These are searched in order: a copy bundled with the app, Homebrew,
//! then the copy inside IINA if that's what this Mac has.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        for path in [
            "@executable_path/../Frameworks",
            "/opt/homebrew/lib",
            "/usr/local/lib",
            "/Applications/IINA.app/Contents/Frameworks",
        ] {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
        }
    }
}
