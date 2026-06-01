use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());

    // 1. `data/` inside this crate's directory — works when the symlink
    //    resolves (macOS/Linux workspace) or when built from the crates.io
    //    package (cargo followed the symlink at publish time).
    let in_crate = manifest.join("data");
    if in_crate.is_dir() {
        println!("cargo:rustc-env=VEROVIO_DATA_DIR={}", in_crate.display());
        return;
    }

    // 2. Workspace fallback — the data lives in the verovio-sys vendor
    //    tree. Covers Windows git checkouts where the symlink is checked
    //    out as a plain text file instead of an actual symlink.
    let workspace = manifest.join("../verovio-sys/vendor/verovio/data");
    if workspace.is_dir() {
        println!(
            "cargo:rustc-env=VEROVIO_DATA_DIR={}",
            workspace.canonicalize().unwrap().display()
        );
        return;
    }

    panic!(
        "Cannot find Verovio data directory.\n\
         Checked:\n  {}\n  {}\n\
         If building from a git checkout, make sure the verovio-sys \
         submodule is initialized:\n  git submodule update --init --recursive",
        in_crate.display(),
        workspace.display(),
    );
}
