# The members' runner test: a job runs in a sandbox that reaches nothing on the box.

start_all()
box.wait_for_unit("docker.service")
other.wait_for_unit("ours.service")
box.wait_for_unit("ours.service")
box.succeed(f"docker load < {nix['probe']}")
assert "runsc" in box.succeed("docker info --format '{{.DefaultRuntime}}'")

run = lambda cmd: box.execute(f"docker run --rm probe:1 /bin/sh -c '{cmd}'")

# the kernel a job sees is gVisor's
status, out = run("uname -a")
assert "gVisor" in out or "4.4.0" in out, out

# nothing of ours answers: the box itself, and another box on the lan
for target in ("192.168.1.1", "192.168.1.2"):
    status, out = run(f"curl -sS -m 5 http://{target}:8080/ >/dev/null; echo rc=$?")
    assert "rc=0" not in out, f"{target} was reachable from a job: {out}"

# and the tailnet range is dropped before it leaves
status, out = run("curl -sS -m 5 http://100.100.100.100/ >/dev/null; echo rc=$?")
assert "rc=0" not in out, out

# names are asked of the box's own nameservers, not docker's embedded
# one, which gVisor cannot reach
status, out = run("cat /etc/resolv.conf")
assert "127.0.0.11" not in out, out

# the job cannot see the host's files or its docker
status, out = run("ls /nix/store | head -1; ls /var/run/docker.sock; echo done")
assert "docker.sock" not in out.replace("cannot access", ""), out
