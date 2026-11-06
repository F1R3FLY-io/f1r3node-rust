{
  description = "RNode - F1R3FLY.io Blockchain Platform Development Environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/146b38388eed8869537500aead1727c18d251349";
    flake-utils.url = "github:numtide/flake-utils";
    sandbox-nix.url = "github:rmgaray/sandbox-nix";
  };

  outputs = { self, nixpkgs, flake-utils, sandbox-nix }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };

        libtorch = pkgs.fetchzip {
          url = "https://download.pytorch.org/libtorch/cpu/libtorch-cxx11-abi-shared-with-deps-2.4.0%2Bcpu.zip";
          sha256 = "sha256-8V19wuY1MWaTvmqxKvr4TzUciQim0oz0N4JVMNrLyyA=";
        };

        sandbox = sandbox-nix.packages.${system}.default;

        fhsEnv = pkgs.buildFHSEnv {
          name = "rchain";

          targetPkgs = pkgs: with pkgs; [
            sbt
            libiconv
            gcc
            protobuf
            glibc
            jdk11
            pkg-config
            openssl
            cmake
            rustup
            haskellPackages.BNFC
            git
            jflex
            coursier
            which
            swi-prolog
            python3
          ];

          profile = ''
            export SBT_OPTS="-Xmx4g -Xss2m -Dsbt.supershell=false"
            alias rnode="./node/target/universal/stage/bin/rnode"
            export PATH="$PATH:/usr/bin"
            export LIBTORCH=${libtorch}
            export LD_LIBRARY_PATH=${libtorch}/lib:$LD_LIBRARY_PATH
            export PKG_CONFIG_PATH="$PKG_CONFIG_PATH:${pkgs.openssl.dev}/lib/pkgconfig"
            export PETTA_HOME=$(pwd)/PeTTa
            export SANDBOX_LIB_PATH=${sandbox}/lib/libsandbox.so
          '';
        };

      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [ fhsEnv ];
          shellHook = ''
            echo "Entering RNode FHS environment..."
            exec ${fhsEnv}/bin/rchain
          '';
        };
      }
    );
}
