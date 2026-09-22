# The thanos test: every box keeps its metrics; the observe box sees them all over the bucket.

start_all()
for m in (a, b):
    m.wait_for_unit("garage.service")
    m.wait_for_open_port(3901)
a_id = a.succeed(f"set -a; . {nix['rpc']}; garage node id -q").strip().split("@")[0]
b.succeed(f"set -a; . {nix['rpc']}; garage node connect {a_id}@a:3901")
for m in (a, b):
    m.succeed("systemctl restart garage-setup.service")
# both boxes hold the applied layout before the buckets and keys are made
for m in (a, b):
    m.wait_until_succeeds(f"set -a; . {nix['rpc']}; garage layout show > /tmp/l && grep -q 'Current cluster layout version: [1-9]' /tmp/l")
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
