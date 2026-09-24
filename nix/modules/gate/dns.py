#!/usr/bin/env python3
# The fleet's names at Cloudflare, converged: `*` and the bare name point at
# the gateway's tailnet address, and each public host is a proxied CNAME to
# the tunnel while the door is open. Only records this script names are
# touched; anything else in the zone is left alone. Environment: the token,
# ZONE, TAILNET, TUNNEL, HOSTS (space separated), PUBLIC (true/false).
import json
import os
import sys
import urllib.request

API = "https://api.cloudflare.com/client/v4"
token = os.environ["CF_DNS_API_TOKEN"]
zone = os.environ["ZONE"]
tailnet = os.environ["TAILNET"]
tunnel = os.environ["TUNNEL"]
hosts = os.environ["HOSTS"].split()
public = os.environ["PUBLIC"] == "true"


def call(method, path, body=None):
    req = urllib.request.Request(
        API + path,
        method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        d = json.load(r)
    if not d["success"]:
        sys.exit("cloudflare: %s %s: %s" % (method, path, d["errors"]))
    return d["result"]


zone_id = call("GET", "/zones?name=" + zone)[0]["id"]
have = {}
for r in call("GET", "/zones/%s/dns_records?per_page=500" % zone_id):
    have.setdefault(r["name"], []).append(r)

want = {
    zone: ("A", tailnet, False),
    "*." + zone: ("A", tailnet, False),
}
for h in hosts:
    if public:
        want[h + "." + zone] = ("CNAME", tunnel + ".cfargotunnel.com", True)
    else:
        want[h + "." + zone] = None

# a name that was public and no longer is: its proxied record goes, so the
# wildcard answers with the tailnet address again
for name, records in have.items():
    if name in want:
        continue
    for r in records:
        if r["type"] == "CNAME" and r["content"].endswith(".cfargotunnel.com"):
            call("DELETE", "/zones/%s/dns_records/%s" % (zone_id, r["id"]))
            print("removed", name, "(no longer public)")

for name, spec in want.items():
    current = have.get(name, [])
    if spec is None:
        for r in current:
            call("DELETE", "/zones/%s/dns_records/%s" % (zone_id, r["id"]))
            print("removed", name)
        continue
    typ, content, proxied = spec
    body = {"type": typ, "name": name, "content": content, "proxied": proxied, "ttl": 1}
    match = [r for r in current if r["type"] == typ]
    for r in current:
        if r["type"] != typ:
            call("DELETE", "/zones/%s/dns_records/%s" % (zone_id, r["id"]))
            print("removed", name, r["type"])
    if not match:
        call("POST", "/zones/%s/dns_records" % zone_id, body)
        print("added", name, typ, content)
    elif match[0]["content"] != content or match[0]["proxied"] != proxied:
        call("PUT", "/zones/%s/dns_records/%s" % (zone_id, match[0]["id"]), body)
        print("updated", name, typ, content)
print("%s: %s" % (zone, "the door is open" if public else "tailnet only"))
