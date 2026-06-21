// The `verovio` crate has no native code of its own, but it owns the test
// binaries that link against the C++ runtime. `cargo:rustc-link-arg` only
// propagates to the targets of the crate that emits it, so the NixOS-specific
// libstdc++ rpath must be re-emitted here for the test binaries to find their
// C++ runtime without LD_LIBRARY_PATH.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Forward verovio-sys's sanitizer mode to this crate's test binaries.
    // `cargo:rustc-link-arg` from a -sys build.rs only applies to its own
    // targets; the safe-wrapper crate has to re-emit for its own.
    match std::env::var("DEP_VEROVIO_SANITIZER").as_deref() {
        Ok("address") => {
            println!("cargo:rustc-link-arg=-fsanitize=address");
            println!("cargo:rustc-link-arg=-fsanitize=undefined");
        }
        Ok("thread") => {
            println!("cargo:rustc-link-arg=-fsanitize=thread");
        }
        _ => {}
    }

    // verovio-sys's native libs are consumed from this crate's tests/examples,
    // so re-emit the C++ runtime link directive here too.
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=dylib=c++");
    } else if cfg!(target_os = "windows") {
        if cfg!(target_env = "gnu") {
            println!("cargo:rustc-link-lib=dylib=stdc++");
        }
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }

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
}
