# Two boxes measure themselves; the observe box answers for both. Each
# sidecar reads its own prometheus and holds a key to the metrics bucket in
# the cluster the two boxes form; the query on a spans a's blocks, a's
# sidecar and b's sidecar, and keeps answering when b is gone.
{ pkgs, self, ... }:
let
  rpc = pkgs.writeText "garage.env" "GARAGE_RPC_SECRET=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n";
  key = box: {
    id = "GK${builtins.substring 0 24 (builtins.hashString "sha256" box)}";
    secret = builtins.hashString "sha256" "${box}-secret";
  };
  objstore =
    box:
    pkgs.writeText "objstore.yaml" ''
      type: S3
      config:
        bucket: metrics
        endpoint: 127.0.0.1:3900
        region: us-east-1
        access_key: ${(key box).id}
        secret_key: ${(key box).secret}
        insecure: true
        signature_version2: false
        bucket_lookup_type: path
    '';
  box = name: {
    imports = [
      ./box.nix
      ../modules/garage.nix
      ../modules/thanos.nix
    ];
    dd.garage = {
      zone = "r-test";
      capacity = "1G";
      dataDir = "/srv/garage";
      publicAddr = "${name}:3901";
      envFile = rpc;
      setupEnvFiles = [
        (pkgs.writeText "metrics-key.env" ''
          GARAGE_METRICS_KEY_ID=${(key name).id}
          GARAGE_METRICS_KEY_SECRET=${(key name).secret}
        '')
      ];
      setup = ''
        garage bucket create metrics 2>/dev/null || true
        garage key import "$GARAGE_METRICS_KEY_ID" "$GARAGE_METRICS_KEY_SECRET" --yes -n metrics-${name} 2>/dev/null || true
        garage bucket allow --read --write metrics --key "$GARAGE_METRICS_KEY_ID" 2>/dev/null || true
      '';
    };
    dd.thanos.objstoreFile = toString (objstore name);
    environment.systemPackages = [ pkgs.curl ];
  };
in
{
  name = "thanos";
  node.specialArgs = { inherit self; };
  defaults.virtualisation.memorySize = 2048;
  defaults.virtualisation.cores = 2; # services start in parallel instead of queueing on one thread
  defaults.virtualisation.diskSize = 4096;
  nodes = {
    a = {
      imports = [
        (box "a")
        {
          # by address, as the fleet does (tailnet ips): the test net has
          # no dns, and grpc's resolver never settles on a bare hostname
          dd.thanos.sidecars = [
            "192.168.1.1:10901"
            "192.168.1.2:10901"
          ];
        }
      ];
    };
    b = box "b";
  };
  testScript = ''
    start_all()
    for m in (a, b):
        m.wait_for_unit("garage.service")
        m.wait_for_open_port(3901)
    a_id = a.succeed("set -a; . ${rpc}; garage node id -q").strip().split("@")[0]
    b.succeed(f"set -a; . ${rpc}; garage node connect {a_id}@a:3901")
    for m in (a, b):
        m.succeed("systemctl restart garage-setup.service")
    a.wait_until_succeeds("set -a; . ${rpc}; garage layout show > /tmp/l && grep -q 'Current cluster layout version: [1-9]' /tmp/l")
    for m in (a, b):
        m.succeed("systemctl restart garage-setup.service")

    # every box: prometheus with the box label, a sidecar that can reach
    # the bucket (it checks at start and fails if it cannot)
    for m in (a, b):
        m.succeed("systemctl start dd-facts.service")
        m.wait_for_unit("prometheus.service")
        m.systemctl("restart thanos-sidecar.service")
        m.wait_for_unit("thanos-sidecar.service")
        m.wait_for_open_port(10901)

    # the observe box: store over the bucket, query over store and sidecars
    a.wait_for_unit("thanos-store.service")
    a.wait_for_unit("thanos-compact.service")
    a.wait_for_unit("thanos-query.service")
    a.wait_for_open_port(10903)
    a.wait_until_succeeds("curl -sf 'http://127.0.0.1:10903/api/v1/query?query=dd_box_cpu_cores' -o /tmp/q && grep -q '\"box\":\"a\"' /tmp/q && grep -q '\"box\":\"b\"' /tmp/q")

    # b goes away: a still answers, with what it has
    b.shutdown()
    a.wait_until_succeeds("curl -sf 'http://127.0.0.1:10903/api/v1/query?query=dd_box_cpu_cores' -o /tmp/q && grep -q '\"box\":\"a\"' /tmp/q")
  '';
}
