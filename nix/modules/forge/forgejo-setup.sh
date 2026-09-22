#!/usr/bin/env bash
# The forge after it starts: the admin, the repo, the branch protection.
set -euo pipefail
api=http://127.0.0.1:$PORT/api/v1
as_admin() { curl -fsS -H "X-WEBAUTH-USER: $ADMIN" "$@"; }
for i in $(seq 1 30); do
  as_admin $api/repos/$ADMIN/Home-Server >/dev/null 2>&1 && break
  sleep 2
done
# the rule, rendered by the module from the flake's own lists
rule="$RULE"
if as_admin $api/repos/$ADMIN/Home-Server/branch_protections/main >/dev/null 2>&1; then
  echo "$rule" | as_admin -X PATCH -H 'content-type: application/json' -d @- $api/repos/$ADMIN/Home-Server/branch_protections/main >/dev/null
  echo "main protection updated"
else
  echo "$rule" | as_admin -X POST -H 'content-type: application/json' -d @- $api/repos/$ADMIN/Home-Server/branch_protections >/dev/null
  echo "main protection created"
fi
