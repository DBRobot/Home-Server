{ pkgs, lib, ... }:
let
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
    after = [ "network-online.target" "zfs-mount.service" ];
    wants = [ "network-online.target" ];
    path = with pkgs; [ curl coreutils ];
    unitConfig.RequiresMountsFor = dir;
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      TimeoutStartSec = "8h"; # 22G over a lossy hotspot
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
    inherit model;
    host = "0.0.0.0";
    port = 8080;
    openFirewall = true;
    extraFlags = [
      "--no-mmap"
      "--mlock" # weights resident; ARC is capped to 8G to leave room
      "-t"
      "4"
      "-c"
      "8192" # bounds the KV cache, which is the real OOM vector
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
