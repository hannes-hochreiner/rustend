{
  description = "Rustend development environment";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, fenix, ... }:
  let
    system = "x86_64-linux";
    name = "rustend";
    pkgs = import nixpkgs {
      inherit system;
      config.allowUnfree = true;
    };
    rustToolchain = with fenix.packages.${system}; combine [
      latest.cargo
      latest.rustc
      latest.rust-analyzer
      latest.rustfmt
      latest.clippy
      # latest.completeToolchain
      targets.wasm32-unknown-unknown.latest.rust-std
    ];
    devDeps = with pkgs; [
      rustToolchain
      wasm-pack
      nushell
      trunk
      openssl
      pkg-config
      bash # default shell for vscode terminal, required as the builder for the dev shell, and for scripts Claude may generate.
      firefox
      geckodriver
      gh
      (vscode-with-extensions.override {
        vscodeExtensions = with pkgs.vscode-extensions; [
          rust-lang.rust-analyzer
          anthropic.claude-code
          streetsidesoftware.code-spell-checker
          fill-labs.dependi
          tamasfe.even-better-toml
          bbenoist.nix
          thenuprojectcontributors.vscode-nushell-lang
        ];
      })
    ];
  in {
    # packages.${system}.hello = nixpkgs.legacyPackages.x86_64-linux.hello;

    # packages.${system}.default = self.packages.x86_64-linux.hello;

    devShells.${system}.default = builtins.derivation {
      inherit name;
      inherit system;
      builder = "${pkgs.bash}/bin/bash";
      __structuredAttrs = true;

      shellHook = ''
        export PATH="${pkgs.lib.makeBinPath devDeps}:$PATH"
        export OPENSSL_DEV=${pkgs.openssl.dev}
        export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig"
        export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath [pkgs.openssl]}:$LD_LIBRARY_PATH"
        export name="${name}"
        exec nu -e "source start.nu"
      '';
    };
  };
}
