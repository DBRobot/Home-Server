# The runner keeps the forge's address from the day it registered; the
# module only re-registers on a new token or labels. A registration at
# another address is dropped so the next step registers again at $URL.
f="$STATE_DIRECTORY/$NAME/.runner"
if [ -e "$f" ] && ! grep -q "\"address\": \"$URL\"" "$f"; then
  echo "runner registered at another address; registering again at $URL"
  rm -f "$f"
fi
