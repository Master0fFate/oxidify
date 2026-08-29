{
  description = "Spotify, native and fast";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # rust-toolchain.toml pins the compiler so local builds and CI agree.
    # This reads that file rather than restating the version here.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ (import rust-overlay) ];
            }
          )
        );
    in
    {
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages =
            with pkgs;
            [
              (rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
              rust-analyzer
              pkg-config
            ]
            ++ lib.optionals stdenv.hostPlatform.isDarwin [
              apple-sdk
            ]
            ++ lib.optionals stdenv.hostPlatform.isLinux [
              alsa-lib
              libpulseaudio
              libxkbcommon
              wayland
              libGL
              xorg.libX11
              xorg.libXcursor
              xorg.libXi
              xorg.libXrandr
            ];
          # The GUI dlopens its Wayland, X11 and GL libraries at run time.
          LD_LIBRARY_PATH = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux (
            pkgs.lib.makeLibraryPath (
              with pkgs;
              [
                libxkbcommon
                wayland
                libGL
                xorg.libX11
                xorg.libXcursor
                xorg.libXi
                xorg.libXrandr
              ]
            )
          );
        };
      });

      packages = forAllSystems (pkgs: rec {
        default = oxidify;
        oxidify =
          let
            toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
            rustPlatform = pkgs.makeRustPlatform {
              cargo = toolchain;
              rustc = toolchain;
            };
            runtimeLibs =
              pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux (
                with pkgs;
                [
                  libxkbcommon
                  wayland
                  libGL
                  xorg.libX11
                  xorg.libXcursor
                  xorg.libXi
                  xorg.libXrandr
                ]
              );
          in
          rustPlatform.buildRustPackage {
            pname = "oxidify";
            version = (pkgs.lib.importTOML ./Cargo.toml).package.version;
            src = self;
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs =
              with pkgs;
              [
                pkg-config
              ]
              ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ makeWrapper ];
            buildInputs =
              pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux (
                with pkgs;
                [
                  alsa-lib
                  libpulseaudio
                ]
              )
              ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.apple-sdk ];

            # The GUI dlopens its Wayland, X11 and GL libraries at run time.
            postFixup = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
              wrapProgram $out/bin/oxidify \
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath runtimeLibs}
            '';

            postInstall = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
              install -Dm644 packaging/applications/oxidify.desktop \
                $out/share/applications/oxidify.desktop
              install -Dm644 packaging/icons/oxidify.svg \
                $out/share/icons/hicolor/scalable/apps/oxidify.svg
            '';

            meta = {
              description = "Fast native Spotify client with local playback and Spotify Connect";
              homepage = "https://github.com/Master0fFate/oxidify";
              license = pkgs.lib.licenses.mit;
              mainProgram = "oxidify";
            };
          };
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt-tree);
    };
}
