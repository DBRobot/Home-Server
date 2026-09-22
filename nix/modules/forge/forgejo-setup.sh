#!/usr/bin/env bash
# The forge after it starts: the admin, the repo, the branch protection.
set -euo pipefail
api=http://127.0.0.1:$PORT/api/v1
as_admin() { curl -fsS -H 'X-WEBAUTH-USER: $ADMIN' "$@"; }
for i in $(seq 1 30); do
  as_admin $api/repos/$ADMIN/Home-Server >/dev/null 2>&1 && break
  sleep 2
done
rule=$(jq -n --arg admin '$ADMIN' '{
  rule_name: "main", branch_name: "main",
  enable_push: true, enable_push_whitelist: true, push_whitelist_usernames: [$admin],
  enable_status_check: true,
  status_check_contexts: [
    "entry_point / flake_check (pull_request)",
    "entry_point / build_client (pull_request)",
    "entry_point / build_host (node1) (pull_request)",
    "entry_point / build_host (node2) (pull_request)",
    "entry_point / lint (pull_request)",
    "entry_point / test (pull_request)",
    "entry_point / vm_tests (directory) (pull_request)",
    "entry_point / vm_tests (metrics) (pull_request)",
    "entry_point / vm_tests (backup) (pull_request)",
    "entry_point / vm_tests (release) (pull_request)",
    "entry_point / vm_tests (thanos) (pull_request)",
    "entry_point / vm_tests (members-runner) (pull_request)",
    "entry_point / vm_tests (games) (pull_request)"
  ],
  block_on_outdated_branch: false,
  required_approvals: 0
}')
if as_admin $api/repos/$ADMIN/Home-Server/branch_protections/main >/dev/null 2>&1; then
  echo "$rule" | as_admin -X PATCH -H 'content-type: application/json' -d @- $api/repos/$ADMIN/Home-Server/branch_protections/main >/dev/null
  echo "main protection updated"
else
  echo "$rule" | as_admin -X POST -H 'content-type: application/json' -d @- $api/repos/$ADMIN/Home-Server/branch_protections >/dev/null
  echo "main protection created"
fi
