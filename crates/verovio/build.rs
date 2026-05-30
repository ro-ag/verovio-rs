// The `verovio` crate has no native code of its own, but it owns the test
// binaries that link against the C++ runtime. `cargo:rustc-link-arg` only
// propagates to the targets of the crate that emits it, so the NixOS-specific
// libstdc++ rpath must be re-emitted here for the test binaries to find their
// C++ runtime without LD_LIBRARY_PATH.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let sanitizer = std::env::var("DEP_VEROVIO_SANITIZER").ok();

    // Forward verovio-sys's sanitizer mode to this crate's test binaries.
    // `cargo:rustc-link-arg` from a -sys build.rs only applies to its own
    // targets; the safe-wrapper crate has to re-emit for its own.
    match sanitizer.as_deref() {
        Some("address") => {
            println!("cargo:rustc-link-arg=-fsanitize=address");
            println!("cargo:rustc-link-arg=-fsanitize=undefined");
        }
        Some("thread") => {
            println!("cargo:rustc-link-arg=-fsanitize=thread");
        }
        _ => {}
    }

    if !cfg!(target_os = "linux") {
        return;
    }

    // Skip the host-libstdc++ rpath in sanitizer builds. Pointing the loader
    // at `/usr/lib/...` (or similar) on the runner pulls in host glibc and
    // crashes the test binary at startup with `undefined symbol:
    // __nptl_change_stack_perm, version GLIBC_PRIVATE` — the symbol exists
    // in the Nix-built sanitizer runtime's expected glibc but was removed
    // from the runner's host glibc 2.34+. Non-sanitized Nix/Linux builds
    // still need the rpath for libstdc++ discovery at runtime.
    if sanitizer.is_some() {
        return;
    }

    let Ok(out) = std::process::Command::new("c++")
        .arg("-print-file-name=libstdc++.so.6")
        .output()
    else {
        return;
    };
    let Ok(path) = std::str::from_utf8(&out.stdout) else {
        return;
    };
    let path = std::path::Path::new(path.trim());
    if !path.is_absolute() {
        return;
    }
    if let Some(libdir) = path.parent() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", libdir.display());
    }
}
