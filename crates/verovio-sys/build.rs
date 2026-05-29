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

    // The include layout mirrors Verovio's own cmake/CMakeLists.txt — every
    // vendored dep sits in its own subdir under include/ and is referenced
    // unqualified by Verovio's source.
    let include_dirs = [
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
    bridge_build.compile("verovio_bridge");

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
