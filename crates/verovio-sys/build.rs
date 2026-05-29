use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let verovio_src = manifest_dir.join("vendor/verovio");

    let cmake_dir = verovio_src.join("cmake");
    if !cmake_dir.join("CMakeLists.txt").exists() {
        panic!(
            "Verovio submodule not initialized at {}.\n\
             Run: git submodule update --init --recursive",
            verovio_src.display()
        );
    }

    // Build Verovio static lib via its own CMake (BUILD_AS_LIBRARY=ON).
    let dst = cmake::Config::new(&cmake_dir)
        .define("BUILD_AS_LIBRARY", "ON")
        .define("CMAKE_BUILD_TYPE", "Release")
        .define("CMAKE_CXX_STANDARD", "20")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("NO_DARMS_SUPPORT", "ON")
        .build();

    // Verovio's CMakeLists builds SHARED (libverovio.so) when BUILD_AS_LIBRARY=ON,
    // installing into $OUT_DIR/lib/. Link dynamically.
    //
    // The rpath the test/bin binaries need (so they find libverovio.so without
    // LD_LIBRARY_PATH) is published via the `links=verovio` metadata channel as
    // DEP_VEROVIO_RPATH for the safe-wrapper crate's build.rs to re-emit —
    // cargo:rustc-link-arg only applies to the emitting crate's own targets,
    // not to downstream binaries.
    let lib_dir = dst.join("lib");
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=verovio");
    println!("cargo:rpath={}", lib_dir.display());

    // Compile the cxx bridge + our shim, including Verovio headers.
    // The include layout mirrors Verovio's own cmake/CMakeLists.txt — every
    // vendored dep sits in its own subdir under include/ and is referenced
    // unqualified by Verovio's source.
    let mut build = cxx_build::bridge("src/lib.rs");
    build
        .file("src/vrv_bridge.cpp")
        .include(verovio_src.join("include"))
        .include(verovio_src.join("include/crc"))
        .include(verovio_src.join("include/midi"))
        .include(verovio_src.join("include/hum"))
        .include(verovio_src.join("include/json"))
        .include(verovio_src.join("include/pugi"))
        .include(verovio_src.join("include/tuning-library"))
        .include(verovio_src.join("include/zip"))
        .include(verovio_src.join("include/vrv"))
        .include(verovio_src.join("libmei/dist"))
        .include(verovio_src.join("libmei/addons"))
        .include(manifest_dir.join("include"))
        .std("c++20")
        .compile("verovio_bridge");

    // C++ runtime. NixOS-specific libstdc++ rpath discovery happens in the
    // verovio safe-wrapper crate's build.rs (rustc-link-arg doesn't propagate
    // from -sys to downstream binaries).
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=dylib=c++");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }

    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/vrv_bridge.cpp");
    println!("cargo:rerun-if-changed=include/vrv_bridge.h");
    println!("cargo:rerun-if-changed=build.rs");
}
