# The forge test: it comes up, its admin exists, main is protected by every ci job.

box.wait_for_unit("forgejo.service")
box.wait_for_open_port(3000)
box.wait_for_unit("forgejo-admin.service")
# with no repository yet, protection has nothing to do and says so
box.wait_for_unit("forgejo-protection.service")
box.succeed("journalctl -u forgejo-protection | grep -q 'nothing to protect'")

# the repository is the one thing the forge does not make for itself (it is
# pushed to); here it is made through the api, then the protection applies
box.succeed(
    "curl -sf -X POST -H 'X-WEBAUTH-USER: %s' -H 'content-type: application/json' "
    "-d '{\"name\":\"Home-Server\",\"default_branch\":\"main\",\"auto_init\":true}' "
    "http://127.0.0.1:3000/api/v1/user/repos >/dev/null" % nix["admin"]
)
box.succeed("systemctl restart forgejo-protection.service")
box.wait_for_unit("forgejo-protection.service")

# the admin, by the name the role gave (this is what the quoting bug broke)
users = box.succeed("curl -sf -H 'X-WEBAUTH-USER: %s' http://127.0.0.1:3000/api/v1/admin/users" % nix["admin"])
assert nix["admin"] in users, users
box.succeed("forgejo admin user list --admin | grep -qw %s" % nix["admin"])

# main is protected, and by the whole suite
prot = box.succeed(
    "curl -sf -H 'X-WEBAUTH-USER: %s' http://127.0.0.1:3000/api/v1/repos/%s/Home-Server/branch_protections/main"
    % (nix["admin"], nix["admin"])
)
import json
p = json.loads(prot)
assert p["enable_status_check"], p
assert any("vm_tests (games)" in c for c in p["status_check_contexts"]), p["status_check_contexts"]
assert p["required_approvals"] == 0
