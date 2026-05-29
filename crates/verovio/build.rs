// The `verovio` crate has no native code of its own, but it owns the test
// binaries that link against the C++ runtime. `cargo:rustc-link-arg` only
// propagates to the targets of the crate that emits it, so the NixOS-specific
// libstdc++ rpath must be re-emitted here for the test binaries to find their
// C++ runtime without LD_LIBRARY_PATH.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if !cfg!(target_os = "linux") {
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

    // Also propagate the libverovio.so rpath that verovio-sys's build.rs
    // emitted into DEP_VEROVIO_RPATH via the `links` metadata channel — see
    // the matching cargo:rpath= line in verovio-sys/build.rs.
    if let Ok(rpath) = std::env::var("DEP_VEROVIO_RPATH") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{rpath}");
    }
}
