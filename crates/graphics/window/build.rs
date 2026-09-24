fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    use std::env;

    let target = env::var("TARGET").unwrap();

    // Needed for TISCopyCurrentKeyboardInputSource and friends, used by the
    // macOS keyboard-layout handling in os/macos/window.rs; unrelated to
    // rendering (the EGL/WGL binding generation that used to live here was
    // removed in task #415 along with the OpenGL/EGL/WGL context code).
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=framework=Carbon");
    }
}
