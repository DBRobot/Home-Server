#!/usr/bin/env bash
# The forge after it starts: the admin, the repo, the branch protection.
set -euo pipefail
api=http://127.0.0.1:$PORT/api/v1
as_admin() { curl -fsS -H "X-WEBAUTH-USER: $ADMIN" "$@"; }
# the repository is pushed to the forge, not made by it: until it is
# there, there is nothing to protect, and this unit will run again
for i in $(seq 1 30); do
  as_admin $api/repos/$ADMIN/Home-Server >/dev/null 2>&1 && break
  sleep 2
done
if ! as_admin $api/repos/$ADMIN/Home-Server >/dev/null 2>&1; then
  echo "no repository $ADMIN/Home-Server yet; nothing to protect"
  exit 0
fi
# the rule, rendered by the module from the flake's own lists
rule="$RULE"
if as_admin $api/repos/$ADMIN/Home-Server/branch_protections/main >/dev/null 2>&1; then
  echo "$rule" | as_admin -X PATCH -H 'content-type: application/json' -d @- $api/repos/$ADMIN/Home-Server/branch_protections/main >/dev/null
  echo "main protection updated"
else
  echo "$rule" | as_admin -X POST -H 'content-type: application/json' -d @- $api/repos/$ADMIN/Home-Server/branch_protections >/dev/null
  echo "main protection created"
fi
# the ci secret the repo's jobs show ci-cancel.py: the value on this box,
# set again each run (the api does not read it back)
printf '{"data":"%s"}' "$(tr -d '\n' < "$CI_SECRET_FILE")" | as_admin -X PUT -H 'content-type: application/json' -d @- $api/repos/$ADMIN/Home-Server/actions/secrets/DD_CI >/dev/null
echo "ci secret set"
