{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  name = "verovio-rs-dev";

  nativeBuildInputs = with pkgs; [
    rustc
    cargo
    rustfmt
    clippy
    rust-analyzer

    clang
    cmake
    pkg-config
    git

    # Build acceleration: sccache caches every translation unit so a clean
    # `cargo clean && cargo build` reuses prior compile results; mold links
    # the final test/bin binary in ~100ms instead of ~10s.
    sccache
    mold
  ];

  buildInputs = with pkgs; [
    libclang
  ];

  # bindgen / cxx need libclang at a discoverable path on NixOS.
  LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";

  # Route both rustc and the C++ compiler invocations through sccache so the
  # ~300 cpp files in Verovio aren't recompiled from scratch on every clean.
  RUSTC_WRAPPER = "sccache";
  CC = "sccache cc";
  CXX = "sccache c++";

  # Use mold as the linker for Rust-produced binaries (test/bin/example).
  # Scoped to the dev shell rather than `.cargo/config.toml` so the crate
  # remains buildable on a host without mold installed (e.g. a crates.io
  # consumer or this same workspace outside `nix-shell`).
  RUSTFLAGS = "-C link-arg=-fuse-ld=mold";

  shellHook = ''
    echo "verovio-rs dev shell:"
    echo "  rustc   $(rustc --version)"
    echo "  clang   $(clang --version | head -1)"
    echo "  cmake   $(cmake --version | head -1)"
    echo "  sccache $(sccache --version) → cache at ''${SCCACHE_DIR:-~/.cache/sccache}"
    echo "  mold    $(mold --version)"
    echo ""
    echo "First-time setup:"
    echo "  git submodule update --init --recursive"
    echo "  cargo test"
  '';
}
