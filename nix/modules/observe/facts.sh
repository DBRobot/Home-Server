#!/usr/bin/env bash
# What this box is, as metrics: cores, memory, disks, site, region.
set -euo pipefail
out=$FACTS/dd_box.prom.tmp
esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }
host=$(uname -n)
model=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')
cores=$(nproc)
mem=$(awk '/MemTotal/ {print $2 * 1024}' /proc/meminfo)
kernel=$(uname -r)
arch=$(uname -m)
gen=$(readlink /run/current-system | sed 's|/nix/store/||' | cut -c1-32)
{
  echo "# HELP dd_box_info What this box is."
  echo "# TYPE dd_box_info gauge"
  echo "dd_box_info{box=\"$(esc "$host")\",model=\"$(esc "$model")\",arch=\"$arch\",kernel=\"$kernel\",generation=\"$gen\",site=\"$SITE\",region=\"$REGION\"} 1"
  echo "# TYPE dd_box_cpu_cores gauge"
  echo "dd_box_cpu_cores{box=\"$(esc "$host")\"} $cores"
  echo "# TYPE dd_box_memory_bytes gauge"
  echo "dd_box_memory_bytes{box=\"$(esc "$host")\"} $mem"
  echo "# TYPE dd_box_cpu_flag gauge"
  for f in avx2 avx512f avx512_vnni amx_tile sha_ni aes; do
    if grep -m1 '^flags' /proc/cpuinfo | grep -qw "$f"; then v=1; else v=0; fi
    echo "dd_box_cpu_flag{box=\"$(esc "$host")\",flag=\"$f\"} $v"
  done
  echo "# TYPE dd_box_disk_bytes gauge"
  lsblk -dbno NAME,SIZE,ROTA,TRAN,TYPE | awk -v h="$(esc "$host")" '$5=="disk" {
    kind = ($3=="1") ? "hdd" : (($4=="nvme") ? "nvme" : "ssd");
    printf "dd_box_disk_bytes{box=\"%s\",device=\"%s\",kind=\"%s\",bus=\"%s\"} %s\n", h, $1, kind, $4, $2 }'
  echo "# TYPE dd_box_plaintext gauge"
  while read -r svc; do [ -n "$svc" ] && echo "dd_box_plaintext{box=\"$(esc "$host")\",service=\"$(esc "$svc")\"} 1"; done < /etc/dd/plaintext
  echo "# TYPE dd_box_gpu_info gauge"
  lspci 2>/dev/null | grep -iE 'vga|3d|display' | sed 's/^[^ ]* //; s/^[^:]*: //' | while read -r g; do
    echo "dd_box_gpu_info{box=\"$(esc "$host")\",name=\"$(esc "$g")\"} 1"
  done
} > "$out"
mv "$out" $FACTS/dd_box.prom
