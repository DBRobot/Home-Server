# Before the runner registers or starts: the forge must answer at $URL with
# a certificate this box believes, and the registration on disk must be
# for that address. A release that renames the forge or renews its
# certificate would otherwise fail this unit and be rolled back for it.
for i in $(seq 1 60); do
  curl -fsS -o /dev/null "$URL/api/v1/version" && break
  echo "waiting for the forge at $URL ($i)"
  sleep 5
done
f="$STATE_DIRECTORY/$NAME/.runner"
if [ -e "$f" ] && ! grep -q "\"address\": \"$URL\"" "$f"; then
  echo "runner registered at another address; registering again at $URL"
  rm -f "$f"
fi
