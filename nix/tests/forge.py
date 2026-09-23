# The forge test: it comes up, its admin exists, main is protected by every ci job.

box.wait_for_unit("forgejo.service")
box.wait_for_open_port(nix["port"])
# the setup units are oneshots: a unit that has not run yet also says
# Result=success, so wait for one that has finished a run
def done(unit):
    box.wait_until_succeeds(
        "[ -n \"$(systemctl show -p ExecMainExitTimestamp --value %s)\" ] && systemctl show -p Result --value %s | grep -qx success" % (unit, unit),
        timeout=180,
    )

done("forgejo-admin.service")
# with no repository yet, protection has nothing to do and says so
done("forgejo-protection.service")
box.succeed("journalctl -u forgejo-protection | grep -q 'nothing to protect'")

# the repository is the one thing the forge does not make for itself (it is
# pushed to); here it is made through the api, then the protection applies
box.succeed(
    "curl -sf -X POST -H 'X-WEBAUTH-USER: %s' -H 'content-type: application/json' "
    "-d '{\"name\":\"Home-Server\",\"default_branch\":\"main\",\"auto_init\":true}' "
    "http://127.0.0.1:%d/api/v1/user/repos >/dev/null" % (nix["admin"], nix["port"])
)
box.succeed("systemctl restart forgejo-protection.service")
done("forgejo-protection.service")

# the admin, by the name the role gave (this is what the quoting bug broke)
users = box.succeed("curl -sf -H 'X-WEBAUTH-USER: %s' http://127.0.0.1:%d/api/v1/admin/users" % (nix["admin"], nix["port"]))
assert nix["admin"] in users, users
assert '"is_admin":true' in users, "the admin is an admin"

# main is protected, and by the whole suite
prot = box.succeed(
    "curl -sf -H 'X-WEBAUTH-USER: %s' http://127.0.0.1:%d/api/v1/repos/%s/Home-Server/branch_protections/main"
    % (nix["admin"], nix["port"], nix["admin"])
)
import json
p = json.loads(prot)
assert p["enable_status_check"], p
assert any("vm_tests (games)" in c for c in p["status_check_contexts"]), p["status_check_contexts"]
assert p["required_approvals"] == 0

# the repo's DD_CI secret exists (its value cannot be read back)
secrets = box.succeed(
    "curl -sf -H 'X-WEBAUTH-USER: %s' http://127.0.0.1:%d/api/v1/repos/%s/Home-Server/actions/secrets"
    % (nix["admin"], nix["port"], nix["admin"])
)
assert '"name":"DD_CI"' in secrets, secrets

# the cancel route: the wrong secret is a 404, the right one reaches the
# forge (a run that does not exist answers 404 there, which comes back as 502)
box.wait_for_open_port(3003)
box.succeed("test $(curl -s -o /dev/null -w '%{http_code}' -X POST -H 'X-DD-CI: wrong' http://127.0.0.1:3003/_dd/ci/cancel/1) = 404")
box.succeed("test $(curl -s -o /dev/null -w '%{http_code}' -X POST -H 'X-DD-CI: test-ci-secret' http://127.0.0.1:3003/_dd/ci/cancel/1) = 502")
