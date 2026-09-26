#!/usr/bin/env python3
# A failing job stops the whole run. Forgejo's api cannot cancel a run, but
# its web route can, and this box is the forge: a job posts here with the
# ci secret, and this cancels the run as the admin through forgejo's socket.
# Environment: FORGE (the unix socket), ADMIN, REPO (owner/name),
# SECRET_FILE, LISTEN (port).
#
# The socket, not a port: forgejo believes X-WEBAUTH-USER from anything that
# reaches it, and jobs run on this box. Only what systemd puts in forgejo's
# group can open it, and a job's user is not in that group.
import http.client
import http.server
import json
import os
import re
import socket

forge = os.environ["FORGE"]
admin = os.environ["ADMIN"]
repo = os.environ["REPO"]
secret = open(os.environ["SECRET_FILE"]).read().strip()


class UnixConnection(http.client.HTTPConnection):
    def __init__(self, path, timeout):
        super().__init__("localhost", timeout=timeout)
        self.unix = path

    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(self.timeout)
        self.sock.connect(self.unix)


def ask(method, path, body=None):
    """forgejo, as the admin, over its socket: (status, body bytes)"""
    c = UnixConnection(forge, 20)
    try:
        c.request(method, path, body=body, headers={"X-WEBAUTH-USER": admin})
        r = c.getresponse()
        return r.status, r.read()
    finally:
        c.close()


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
            status, body = ask("GET", "/api/v1/repos/%s/actions/runs/%s" % (repo, run))
            if status != 200:
                raise ValueError("http %s" % status)
            index = json.loads(body)["html_url"].rstrip("/").rsplit("/", 1)[1]
        except (OSError, KeyError, ValueError) as e:
            print("run %s: not found (%s)" % (run, e), flush=True)
            self.send_response(502)
            self.end_headers()
            return
        try:
            code, _ = ask("POST", "/%s/actions/runs/%s/cancel" % (repo, index), b"")
        except OSError as e:
            print("run %s: %s" % (run, e), flush=True)
            code = 502
        print("run %s (#%s): cancel -> %s" % (run, index, code), flush=True)
        self.send_response(200 if code == 200 else 502)
        self.end_headers()

    def log_message(self, *_):
        pass


http.server.HTTPServer(("127.0.0.1", int(os.environ["LISTEN"])), Handler).serve_forever()
