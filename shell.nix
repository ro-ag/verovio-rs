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
  ];

  buildInputs = with pkgs; [
    libclang
  ];

  # bindgen / cxx need libclang at a discoverable path on NixOS.
  LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";

  shellHook = ''
    echo "verovio-rs dev shell:"
    echo "  rustc  $(rustc --version)"
    echo "  clang  $(clang --version | head -1)"
    echo "  cmake  $(cmake --version | head -1)"
    echo ""
    echo "First-time setup:"
    echo "  git submodule update --init --recursive"
    echo "  cargo test -p verovio --test version"
  '';
}
