{
  description = "Home server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    sops-nix.url = "github:Mic92/sops-nix";
    sops-nix.inputs.nixpkgs.follows = "nixpkgs";
    disko.url = "github:nix-community/disko/latest";
    disko.inputs.nixpkgs.follows = "nixpkgs";
    # rust builds in two layers: every dependency once, cached in the
    # bucket until Cargo.lock changes; our crates on top, a minute
    crane.url = "github:ipetkov/crane";
    # a toolchain with the wasm32 target, for the browser side
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      sops-nix,
      disko,
      crane,
      rust-overlay,
      ...
    }:
    let
      # devShells are per-system; nixosConfigurations are not. Both machines
      # here are x86_64-linux, so one system is enough for now.
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      # every vm test, by name: nix/tests/<name>.nix + .py. Also what the ci
      # matrix runs and what main requires (nix/modules/forge/forgejo.nix)
      vmTests = [
        "directory"
        "metrics"
        "backup"
        "release"
        "thanos"
        "members-runner"
        "games"
        "forge"
        "transcode"
      ];
      rust =
        let
          craneLib = crane.mkLib pkgs;
          # One source tree per binary: the workspace files, every member's
          # manifest (cargo loads the whole workspace), and the full trees
          # of only the crates that binary is built from - with the pages'
          # templates, stylesheets, scripts and icons those include. The
          # members it does not use get an empty stub target, so a change
          # to the verifier's pages is not a games rebuild, not a games vm
          # test, not a release of anything but the verifier. The filter
          # step is content-addressed; the stub step only sees its output.
          srcFor =
            name: dirs:
            let
              filtered = pkgs.lib.cleanSourceWith {
                src = ./.;
                filter =
                  path: type:
                  let
                    rel = pkgs.lib.removePrefix (toString ./. + "/") (toString path);
                    top = builtins.elem rel [
                      "Cargo.toml"
                      "Cargo.lock"
                    ];
                    # the trees on the way to every member, and every manifest
                    tree =
                      builtins.elem rel [
                        "box"
                        "client"
                        "app"
                      ]
                      || builtins.match "(box|client)/[^/]+" rel != null;
                    manifest = builtins.match "(box|client)/[^/]+/Cargo\\.toml" rel != null || rel == "app/Cargo.toml";
                    # the app's pages, icons and tauri config sit beside its src
                    appFile = pkgs.lib.hasPrefix "app/" rel;
                    ours = builtins.any (d: rel == d || pkgs.lib.hasPrefix "${d}/" rel) dirs;
                  in
                  top
                  || manifest
                  || (type == "directory" && tree)
                  || (
                    ours
                    && (
                      type == "directory"
                      || craneLib.filterCargoSources path type
                      || builtins.match ".*/(templates|web)/.*" rel != null
                      || appFile
                    )
                  );
              };
            in
            pkgs.runCommand "src-${name}" { } ''
              cp -r ${filtered} $out
              chmod -R u+w $out
              for m in $out/box/* $out/client/* $out/app; do
                [ -f "$m/Cargo.toml" ] || continue
                if [ ! -d "$m/src" ]; then
                  mkdir -p "$m/src"
                  : > "$m/src/lib.rs"
                  echo 'fn main() {}' > "$m/src/main.rs"
                fi
              done
            '';
          # every crate: the checks (fmt, clippy, tests) build the workspace
          src = srcFor "workspace" [
            "box"
            "client"
            "app"
          ];
          # the crates behind each binary: the path dependencies in the
          # manifests, followed to the end, so a list never goes stale
          crateDeps =
            dir:
            let
              m = builtins.fromTOML (builtins.readFile (./. + "/${dir}/Cargo.toml"));
              deps = (m.dependencies or { }) // (m.build-dependencies or { });
              norm =
                base: rel:
                let
                  parts = pkgs.lib.filter (p: p != "" && p != ".") (pkgs.lib.splitString "/" "${base}/${rel}");
                  walk =
                    acc: ps:
                    if ps == [ ] then
                      acc
                    else if builtins.head ps == ".." then
                      walk (pkgs.lib.init acc) (builtins.tail ps)
                    else
                      walk (acc ++ [ (builtins.head ps) ]) (builtins.tail ps);
                in
                pkgs.lib.concatStringsSep "/" (walk [ ] parts);
            in
            pkgs.lib.unique (
              map (d: norm dir d.path) (pkgs.lib.filter (d: d ? path) (builtins.attrValues deps))
            );
          crateClosure =
            roots:
            let
              go =
                seen: todo:
                if todo == [ ] then
                  seen
                else
                  let
                    d = builtins.head todo;
                    rest = builtins.tail todo;
                  in
                  if builtins.elem d seen then go seen rest else go (seen ++ [ d ]) (rest ++ crateDeps d);
            in
            go [ ] roots;
          crateSrc = name: roots: srcFor name (crateClosure roots);
          sources = {
            dd = crateSrc "dd" [
              "client/cli"
              "client/gitremote"
            ];
            agent = crateSrc "agent" [ "box/release" ];
            verify = crateSrc "verify" [ "box/verify" ];
            games = crateSrc "games" [ "box/games" ];
            transcode = crateSrc "transcode" [ "box/transcode" ];
            web = crateSrc "web" [ "client/web" ];
            app = crateSrc "app" [ "app" ];
          };
          # what the app's window is made of; the workspace checks compile
          # the app too, so they need it as well
          appLibs = [
            pkgs.webkitgtk_4_1
            pkgs.gtk3
            pkgs.libsoup_3
            pkgs.glib
            pkgs.openssl
          ];
          # the same toolchain, plus the wasm32 target; nixpkgs' rustc
          # ships no std for it
          wasmToolchain =
            (pkgs.extend rust-overlay.overlays.default).rust-bin.stable.latest.minimal.override
              {
                targets = [ "wasm32-unknown-unknown" ];
              };
          craneWasm = craneLib.overrideToolchain wasmToolchain;
          wasmCommon = {
            src = sources.web;
            strictDeps = true;
            doCheck = false;
            CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
            cargoExtraArgs = "-p dd-web";
          };
          wasmArtifacts = craneWasm.buildDepsOnly (
            wasmCommon
            // {
              pname = "dd-web-deps";
              version = "0.1.0";
            }
          );
          wasmBuild = craneWasm.buildPackage (
            wasmCommon
            // {
              pname = "dd-web";
              version = "0.1.0";
              cargoArtifacts = wasmArtifacts;
            }
          );
          common = {
            inherit src;
            strictDeps = true;
            nativeBuildInputs = [ pkgs.pkg-config ];
            # the cli's mount (dd media) links libfuse; the app its window
            buildInputs = [ pkgs.fuse3 ] ++ appLibs;
            doCheck = false;
          };
          # the dependencies for the whole workspace: what the checks
          # (fmt, clippy, tests) build on
          cargoArtifacts = craneLib.buildDepsOnly (
            common
            // {
              pname = "dd-deps";
              version = "0.1.0";
            }
          );
          # and one dependency build per binary, with that binary's own
          # `-p`: cargo unifies features per package set, so a cache built
          # for the whole workspace does not match `-p dd` and every run
          # recompiled rustic and its friends only to throw them away
          crate =
            pname: source: cargoExtraArgs:
            let
              c = common // {
                src = source;
              };
            in
            craneLib.buildPackage (
              c
              // {
                inherit pname cargoExtraArgs;
                version = "0.1.0";
                cargoArtifacts = craneLib.buildDepsOnly (
                  c
                  // {
                    inherit cargoExtraArgs;
                    pname = "${pname}-deps";
                    version = "0.1.0";
                  }
                );
              }
            );
        in
        {
          # the cli, and `git remote add origin dd::...`, which dd repo calls too
          dd = crate "dd" sources.dd "-p dd -p git-remote-dd";
          # The release agent, on every box. Server side like verify: the
          # release crate holds the file format the cli signs and this
          # binary checks, and nothing that needs a keyring.
          agent = crate "dd-agent" sources.agent "-p release";
          # The verifier behind nginx's auth_request. Built separately from dd
          # rather than as another binary in the same derivation: this one
          # runs on a server and has no business pulling in the keyring/dbus
          # stack that the cli needs.
          verify = crate "verify" sources.verify "-p verify";
          # the manager behind the Games tile (modules/games.nix)
          games = crate "dd-games" sources.games "-p games";
          # one file, one viewer, in memory: the compute side of the
          # encrypted libraries (modules/library/transcode.nix)
          transcode = crate "dd-transcode" sources.transcode "-p transcode";
          # The app: the same crates as dd behind a window (app/). Wrapped
          # so the webview finds its schemas and gio modules; the dmabuf
          # renderer is off because on nvidia it draws a blank window.
          # Jellyfin comes with it: Movies & TV starts it on the device.
          app = (crate "commonty" sources.app "-p commonty").overrideAttrs (old: {
            nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ pkgs.wrapGAppsHook3 ];
            buildInputs = (old.buildInputs or [ ]) ++ [ pkgs.glib-networking ];
            postFixup =
              (old.postFixup or "")
              + "\n"
              + ''
                wrapProgram $out/bin/commonty \
                  --set WEBKIT_DISABLE_DMABUF_RENDERER 1 \
                  --set COMMONTY_JELLYFIN ${pkgs.jellyfin}/bin/jellyfin
              '';
          });
          # Our Rust in the browser: the ente account for a person whose key
          # is a passkey (crates/web). The verifier serves this directory.
          web = pkgs.runCommand "dd-web-dist" { nativeBuildInputs = [ pkgs.wasm-bindgen-cli ]; } ''
            mkdir -p $out
            wasm-bindgen --target web --no-typescript --out-dir $out ${wasmBuild}/lib/dd_web.wasm
          '';

          # the checks, on the same compiled artifacts as the binaries: fmt
          # and clippy in seconds, the tests once, all cached by content and
          # shared between the runner boxes through the bucket. ci builds
          # these instead of running cargo a second and third time.
          fmt = craneLib.cargoFmt {
            inherit src;
            pname = "dd";
            version = "0.1.0";
          };
          clippy = craneLib.cargoClippy (
            common
            // {
              inherit cargoArtifacts;
              pname = "dd";
              version = "0.1.0";
              cargoClippyExtraArgs = "--all-targets -- -D warnings";
            }
          );
          tests = craneLib.cargoTest (
            common
            // {
              inherit cargoArtifacts;
              pname = "dd";
              version = "0.1.0";
              # the e2e tests spawn verifiers on localhost and run git
              nativeBuildInputs = [
                pkgs.pkg-config
                pkgs.gitMinimal
              ];
            }
          );
        };
    in
    {
      # `nix develop` drops you into a shell with the rust toolchain on PATH.
      # Nothing is installed globally and any machine cloning this repo gets
      # exactly these versions.
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          pkgs.cargo
          pkgs.rustc
          pkgs.rust-analyzer # editor: completion, jump to definition
          pkgs.clippy # linter that teaches you the language
          pkgs.rustfmt
          pkgs.pkg-config # crates with C dependencies need this to find them
          pkgs.fuse3 # the cli's mount
          pkgs.sops # `dd secret run -- ...` runs this
          pkgs.age
          pkgs.cargo-tauri # `cargo tauri dev` in app/
        ]
        ++ [
          # the app's window, for `cargo build -p commonty` here
          pkgs.webkitgtk_4_1
          pkgs.gtk3
          pkgs.libsoup_3
          pkgs.glib
          pkgs.openssl
          pkgs.glib-networking
        ];
        RUST_BACKTRACE = "1";
        # the webview finds its gio modules (tls) and, on nvidia, draws
        WEBKIT_DISABLE_DMABUF_RENDERER = "1";
        GIO_MODULE_DIR = "${pkgs.glib-networking}/lib/gio/modules";
      };

      # `nix build .#dd` / `nix run .#dd -- status`. Every dependency is
      # fetched by hash from Cargo.lock, so the binary is as reproducible as
      # the nixos closure. crane builds the dependencies as their own
      # derivation, so a change to our code costs our code's compile, not
      # the three hundred crates under it. The tests ran already in ci's
      # rust job on this same source; no second run in here.
      packages.${system} = builtins.removeAttrs rust [
        "fmt"
        "clippy"
        "tests"
      ];

      # Boxes booted as vms and driven through the failure cases, so the
      # modules the real hosts import are proven before a host sees them.
      # `nix build .#checks.x86_64-linux.directory`; ci runs them when the
      # modules or the verifier change. Not part of a plain `nix flake check`
      # run's build set on a laptop without kvm: `--no-build` there.
      checks.${system} =
        let
          args = {
            inherit pkgs self;
            lib = nixpkgs.lib;
          };
          vm =
            name:
            (import ./nix/lib/vm-test.nix {
              inherit pkgs vmTests;
              lib = nixpkgs.lib;
              boxNames = builtins.attrNames (builtins.fromJSON (builtins.readFile ./fleet/boxes.json));
            })
              name
              (import ./nix/tests/${name}.nix args);
        in
        {
        }
        // nixpkgs.lib.genAttrs vmTests vm
        // {
          inherit (rust) fmt clippy tests;
          placement = import ./nix/tests/placement.nix args;
          ci = import ./nix/tests/ci.nix (args // { inherit vmTests; });
          boxes = import ./nix/tests/boxes.nix args;
        };

      # One box per entry in fleet/boxes.json: its hardware file plus its
      # roles. What a box runs is data, not a hand-written import list, and
      # the per-box wiring (who its peers are, which prometheus grafana reads)
      # is derived from the same list.
      nixosConfigurations =
        let
          boxes = builtins.fromJSON (builtins.readFile ./fleet/boxes.json);
          lib = nixpkgs.lib;
          directoryOf =
            name: box:
            if box.public then
              "https://files.commonty.org/_dd/directory"
            else
              "http://${box.tailnet}:4181/_dd/directory";
          # a role that declares secrets imports roles/_sops.nix; the sops
          # module has to be present for it. Roles are files; look inside.
          needsSops =
            roles: builtins.any (r: lib.hasInfix "_sops.nix" (builtins.readFile ./nix/roles/${r}.nix)) roles;
          storage = lib.filterAttrs (_: b: builtins.elem "storage" b.roles) boxes;
          # the garage cluster: every storage box, each told about the others
          # whose ids are known (a box's id exists once it has started once)
          garageOf = name: box: {
            dd.garage = {
              zone = box.regionId;
              inherit (box.garage) capacity dataDir;
              publicAddr = "${box.tailnet}:3901";
              peers = lib.mapAttrsToList (_: b: "${b.garage.id}@${b.tailnet}:3901") (
                lib.filterAttrs (n: b: n != name && b.garage ? id) storage
              );
            };
          };
          # closures come from the nix-cache bucket in the garage cluster:
          # through the box's own garage, or a storage box's over the tailnet
          cacheOf =
            name: box:
            let
              host =
                if builtins.elem "storage" box.roles then
                  "127.0.0.1"
                else
                  (builtins.head (builtins.attrValues storage)).tailnet;
            in
            {
              dd.agent.cache = "s3://nix-cache?endpoint=${host}:3900&scheme=http&region=us-east-1";
            };
          # a box without garage backs up to a storage box over the tailnet
          backupEndpoint =
            name: box:
            lib.optionalAttrs (!(builtins.elem "storage" box.roles)) {
              dd.backup.endpoint = "http://${(builtins.head (builtins.attrValues storage)).tailnet}:3900";
            };
          mkBox =
            name: box:
            lib.nixosSystem {
              specialArgs = {
                inherit self vmTests;
                boxNames = builtins.attrNames boxes;
                # a unit's script from a file beside its module (nix/lib/script.nix)
                ddScript = import ./nix/lib/script.nix { lib = nixpkgs.lib; };
              };
              modules = [
                ./nix/hosts/${name}/hardware.nix
                {
                  networking.hostName = name;
                  dd.box.tailnet = box.tailnet;
                  dd.box.site = box.siteId;
                  dd.box.region = box.regionId;
                  dd.verify.peers = lib.mapAttrsToList directoryOf (lib.filterAttrs (n: _: n != name) boxes);
                }
              ]
              ++ map (r: ./nix/roles/${r}.nix) box.roles
              ++ lib.optional (builtins.elem "storage" box.roles) (garageOf name box)
              ++ [
                (backupEndpoint name box)
                (cacheOf name box)
              ]
              ++ lib.optional (needsSops box.roles) sops-nix.nixosModules.sops
              ++ lib.optional (builtins.pathExists ./nix/hosts/${name}/disko.nix) disko.nixosModules.disko
              ++ lib.optional (builtins.elem "observe" box.roles) {
                dd.grafana.boxes = lib.mapAttrs (
                  n: b: if n == name then "http://127.0.0.1:9090" else "http://${b.tailnet}:9090"
                ) boxes;
                # every box's sidecar, for the fleet-wide query
                dd.thanos.sidecars = lib.mapAttrsToList (
                  n: b: if n == name then "127.0.0.1:10901" else "${b.tailnet}:10901"
                ) boxes;
              };
            };
        in
        lib.mapAttrs mkBox boxes;
    };
}
