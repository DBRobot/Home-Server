{
  config,
  pkgs,
  lib,
  ...
}:
let
  # nixpkgs ships a baseline x86-64 build so it runs anywhere, which means no
  # SIMD at all: system_info reported LLAMAFILE/OPENMP/REPACK and no AVX line,
  # while this CPU has avx512f/bw/vl + avx512_vnni. Prefill was running scalar.
  # Hardcoding -DGGML_AVX512=ON does not work: the derivation runs llama-server
  # at build time to generate shell completions, so it SIGILLs on any builder
  # without AVX-512 (GitHub runners are a mix). ALL_VARIANTS builds every
  # microarchitecture as a loadable backend and picks the best at runtime, so
  # the build is portable and node1 still gets the icelake path.
  llamaCppTuned = pkgs.llama-cpp.overrideAttrs (o: {
    cmakeFlags = (o.cmakeFlags or [ ]) ++ [
      "-DGGML_NATIVE=OFF"
      "-DGGML_BACKEND_DL=ON"
      "-DGGML_CPU_ALL_VARIANTS=ON"
    ];
  });

  repo = "unsloth/Qwen3.6-35B-A3B-GGUF";
  rev = "a483e9e6cbd595906af30beda3187c2663a1118c";
  upstream = "Qwen3.6-35B-A3B-UD-Q4_K_XL.gguf";
  sha256 = "707a55a8a4397ecde44de0c499d3e68c1ad1d240d1da65826b4949d1043f4450";

  dir = "/tank/models";
  model = "${dir}/qwen3.6-35b-a3b-${lib.substring 0 8 rev}-UD-Q4_K_XL.gguf";
  url = "https://huggingface.co/${repo}/resolve/${rev}/${upstream}";
in
{
  systemd.services.fetch-model = {
    description = "Fetch ${upstream} into ${dir}";
    after = [
      "network-online.target"
      "zfs-mount.service"
    ];
    wants = [ "network-online.target" ];
    path = with pkgs; [
      curl
      coreutils
    ];
    unitConfig.RequiresMountsFor = dir;
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      TimeoutStartSec = "8h"; # 22G over a lossy hotspot
      Restart = "on-failure";
      RestartSec = 60;
      IOSchedulingClass = "idle";
      ExecStartPost = "${pkgs.systemd}/bin/systemctl start --no-block llama-cpp.service";
    };
    script = ''
      set -uo pipefail
      [ -e ${model} ] && exit 0
      mkdir -p ${dir}
      for attempt in $(seq 1 60); do
        echo "fetch attempt $attempt"
        curl -fL --retry 5 --retry-delay 10 --retry-all-errors \
          -C - -o ${model}.part ${url} && break
        sleep 20
      done
      echo "${sha256}  ${model}.part" | sha256sum -c -
      chmod 644 ${model}.part
      mv ${model}.part ${model}
    '';
  };

  # a timer, not wantedBy multi-user.target: a Type=oneshot on the activation
  # path would block nixos-rebuild for the whole download
  systemd.timers.fetch-model = {
    wantedBy = [ "timers.target" ];
    timerConfig = {
      OnActiveSec = "5s";
      OnBootSec = "1min";
      AccuracySec = "1s";
    };
  };

  services.llama-cpp = {
    enable = true;
    package = llamaCppTuned;
    inherit model;
    # localhost only: litellm in modules/litellm.nix is the front door and
    # does the kanidm jwt check. Before this, 8081 was the one service open
    # on every interface with no authentication at all.
    host = "127.0.0.1";
    port = 8081; # 8080 is taken by ente's museum
    openFirewall = false;
    extraFlags = [
      "--no-mmap"
      "--mlock" # weights resident; ARC is capped to 8G to leave room
      "-t"
      "8" # llama-bench: 5.00 vs 4.82 tok/s at 4 threads
      "-c"
      "8192" # bounds the KV cache, which is the real OOM vector
      "--reasoning"
      "off" # default off: measured 0.8s vs 16.5s for the same answer.
      # per-request opt-in with chat_template_kwargs {"enable_thinking": true}
    ];
  };

  systemd.services.llama-cpp = {
    after = [ "fetch-model.service" ];
    unitConfig.ConditionPathExists = model; # skip cleanly until the fetch lands
    serviceConfig = {
      LimitMEMLOCK = "infinity";
      MemoryHigh = "28G";
      MemoryMax = "32G";
    };
  };
}
