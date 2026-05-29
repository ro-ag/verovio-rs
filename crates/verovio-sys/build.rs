use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let verovio_src = manifest_dir.join("vendor/verovio");

    if !verovio_src.join("include/vrv/toolkit.h").exists() {
        panic!(
            "Verovio submodule not initialized at {}.\n\
             Run: git submodule update --init --recursive",
            verovio_src.display()
        );
    }

    // Verovio's src/vrv.cpp does `#include "git_commit.h"` on non-Windows non-
    // CocoaPods non-SwiftPackage builds. Upstream's `tools/get_git_commit.sh`
    // writes a header that appends `-<short-sha>` to `Toolkit::GetVersion()`;
    // since we pin Verovio by tag, the hash adds no information for our
    // users — leaving GIT_COMMIT empty makes `tk.version()` return a clean
    // `"6.2.1"` instead of `"6.2.1[verovio-rs]"` or `"6.2.1-8d42439dc"`.
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out_dir.join("git_commit.h"), "#define GIT_COMMIT \"\"\n")
        .expect("write git_commit.h");

    // The include layout mirrors Verovio's own cmake/CMakeLists.txt — every
    // vendored dep sits in its own subdir under include/ and is referenced
    // unqualified by Verovio's source.
    let include_dirs = [
        out_dir.clone(), // first, so our generated git_commit.h wins
        verovio_src.join("include"),
        verovio_src.join("include/crc"),
        verovio_src.join("include/midi"),
        verovio_src.join("include/hum"),
        verovio_src.join("include/json"),
        verovio_src.join("include/pugi"),
        verovio_src.join("include/tuning-library"),
        verovio_src.join("include/zip"),
        verovio_src.join("include/vrv"),
        verovio_src.join("libmei/dist"),
        verovio_src.join("libmei/addons"),
        manifest_dir.join("include"),
    ];

    // Sanitizer feature gates. These are mutually exclusive — ASan and TSan
    // share runtime state in libsanitizer and can't be linked together.
    let asan = std::env::var_os("CARGO_FEATURE_SANITIZE").is_some();
    let tsan = std::env::var_os("CARGO_FEATURE_SANITIZE_THREAD").is_some();
    assert!(
        !(asan && tsan),
        "verovio-sys: `sanitize` and `sanitize-thread` features are mutually exclusive"
    );
    let sanitizer_flags: Vec<&str> = if asan {
        vec![
            "-fsanitize=address",
            "-fsanitize=undefined",
            "-fno-omit-frame-pointer",
        ]
    } else if tsan {
        vec!["-fsanitize=thread", "-fno-omit-frame-pointer"]
    } else {
        vec![]
    };

    // Compile Verovio + libmei into a single static archive. We skip
    // tools/c_wrapper.cpp deliberately: we bridge to the C++ Toolkit directly
    // via cxx, and the C wrapper isn't part of our linkage path.
    let mut verovio_build = cc::Build::new();
    verovio_build
        .cpp(true)
        .std("c++20")
        .warnings(false) // upstream Verovio has unused-parameter warnings; not ours to fix
        .flag_if_supported("-fvisibility=hidden")
        .flag_if_supported("-fvisibility-inlines-hidden");
    for f in &sanitizer_flags {
        verovio_build.flag(f);
    }

    for dir in &include_dirs {
        verovio_build.include(dir);
    }

    for sub in [
        "src",
        "src/crc",
        "src/hum",
        "src/midi",
        "src/pugi",
        "src/json",
        "libmei/dist",
        "libmei/addons",
    ] {
        add_cpp_sources(&mut verovio_build, &verovio_src.join(sub));
    }

    verovio_build.compile("verovio"); // produces libverovio.a, emits rustc-link-lib=static=verovio

    // Compile the cxx bridge + our shim against the same include layout.
    let mut bridge_build = cxx_build::bridge("src/lib.rs");
    bridge_build
        .file("src/vrv_bridge.cpp")
        .std("c++20")
        .warnings(false);
    for dir in &include_dirs {
        bridge_build.include(dir);
    }
    for f in &sanitizer_flags {
        bridge_build.flag(f);
    }
    bridge_build.compile("verovio_bridge");

    // Emit sanitizer link args for verovio-sys's own benchmarks/binaries
    // /tests. The verovio crate's build.rs re-emits the same flags for the
    // downstream targets (`cargo:rustc-link-arg` is per-package).
    for f in &sanitizer_flags {
        // -fno-omit-frame-pointer is a compile-only flag; skip at link.
        if f.starts_with("-fsanitize=") {
            println!("cargo:rustc-link-arg={f}");
        }
    }

    // Publish the active sanitizer mode to downstream build scripts via
    // the `links=verovio` metadata channel (DEP_VEROVIO_SANITIZER).
    let sanitizer_mode = if asan {
        "address"
    } else if tsan {
        "thread"
    } else {
        "none"
    };
    println!("cargo:sanitizer={sanitizer_mode}");

    // C++ runtime.
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=dylib=c++");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
        emit_libstdcxx_rpath_for_own_tests();
    }

    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/vrv_bridge.cpp");
    println!("cargo:rerun-if-changed=include/vrv_bridge.h");
    println!("cargo:rerun-if-changed=build.rs");
}

/// On NixOS the C++ runtime sits under `/nix/store/<hash>-gcc-*-lib/lib` with
/// no FHS path. Discover it via the host compiler and emit an rpath that
/// applies to this crate's own benchmarks/binaries/examples/tests.
///
/// `cargo:rustc-link-arg` propagates only to targets in the same package as
/// the emitting build.rs — the `verovio` safe-wrapper crate emits the same
/// rpath for its own targets in its own build.rs.
fn emit_libstdcxx_rpath_for_own_tests() {
    let Ok(out) = std::process::Command::new("c++")
        .arg("-print-file-name=libstdc++.so.6")
        .output()
    else {
        return;
    };
    let Ok(path) = std::str::from_utf8(&out.stdout) else {
        return;
    };
    let path = Path::new(path.trim());
    if !path.is_absolute() {
        return;
    }
    if let Some(libdir) = path.parent() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", libdir.display());
    }
}

fn add_cpp_sources(build: &mut cc::Build, dir: &Path) {
    for entry in std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir({}) failed: {}", dir.display(), e))
    {
        let path = entry.expect("dir entry").path();
        let ext = path.extension().and_then(|s| s.to_str());
        // jsonxx upstream uses .cc; everything else is .cpp.
        if matches!(ext, Some("cpp") | Some("cc")) {
            build.file(&path);
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}
