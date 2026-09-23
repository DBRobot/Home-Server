#!/usr/bin/env python3
# A failing job stops the whole run. Forgejo's api cannot cancel a run, but
# its web route can, and this box is the forge: a job posts here with the
# ci secret, and this cancels the run as the admin through the internal
# listener. Environment: FORGE (http://127.0.0.1:port), ADMIN, REPO
# (owner/name), SECRET_FILE, LISTEN (port).
import http.server
import json
import os
import re
import urllib.error
import urllib.request

forge = os.environ["FORGE"]
admin = os.environ["ADMIN"]
repo = os.environ["REPO"]
secret = open(os.environ["SECRET_FILE"]).read().strip()


class Handler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        m = re.fullmatch(r"/_dd/ci/cancel/(\d+)", self.path)
        if not m or self.headers.get("X-DD-CI") != secret:
            self.send_response(404)
            self.end_headers()
            return
        run = m.group(1)
        # a job knows the run's id; the web route wants its number in the
        # repository, which the api gives as the tail of the run's page
        try:
            with urllib.request.urlopen(
                urllib.request.Request(
                    "%s/api/v1/repos/%s/actions/runs/%s" % (forge, repo, run),
                    headers={"X-WEBAUTH-USER": admin},
                ),
                timeout=20,
            ) as r:
                index = json.load(r)["html_url"].rstrip("/").rsplit("/", 1)[1]
        except (urllib.error.HTTPError, KeyError, ValueError) as e:
            print("run %s: not found (%s)" % (run, e), flush=True)
            self.send_response(502)
            self.end_headers()
            return
        req = urllib.request.Request(
            "%s/%s/actions/runs/%s/cancel" % (forge, repo, index),
            method="POST",
            data=b"",
            headers={"X-WEBAUTH-USER": admin},
        )
        try:
            with urllib.request.urlopen(req, timeout=20) as r:
                code = r.status
        except urllib.error.HTTPError as e:
            code = e.code
        print("run %s (#%s): cancel -> %s" % (run, index, code), flush=True)
        self.send_response(200 if code == 200 else 502)
        self.end_headers()

    def log_message(self, *_):
        pass


http.server.HTTPServer(("127.0.0.1", int(os.environ["LISTEN"])), Handler).serve_forever()
