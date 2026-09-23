# The transcode test: a box plays a file it cannot read at rest.
import json

box.wait_for_unit("dd-transcode.service")
box.wait_for_open_port(4190)
# a clip big enough that ffmpeg must seek for the index at its end, as a real file makes it
box.succeed("ffmpeg -hide_banner -loglevel error -f lavfi -i testsrc=duration=20:size=640x360:rate=25 -f lavfi -i sine=frequency=440:duration=20 -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest /tmp/clip.mp4")
# the box's key for this run, then the clip chunked and sealed the way a device does it
key = json.loads(box.succeed("curl -sf http://127.0.0.1:4190/key"))["key"]
body = box.succeed("python3 %s /tmp/clip.mp4 /tmp/chunks '%s' http://127.0.0.1:8000" % (nix["chunker"], key)).strip()
box.succeed("cd /tmp/chunks && (python3 -m http.server 8000 >/dev/null 2>&1 &) && sleep 1")
box.succeed("printf '%s' '%s' > /tmp/body.json" % ("%s", body.replace("'", "'\\''")))
started = json.loads(box.succeed("curl -sf -X POST -H 'content-type: application/json' -d @/tmp/body.json http://127.0.0.1:4190/session"))
sid = started["id"]
# the playlist grows as ffmpeg works; a segment is playable bytes
box.wait_until_succeeds("curl -sf http://127.0.0.1:4190/session/%s/index.m3u8 | grep -q '\\.ts'" % sid, timeout=120)
seg = box.succeed("curl -sf http://127.0.0.1:4190/session/%s/index.m3u8 | grep '\\.ts' | head -1" % sid).strip()
box.succeed("curl -sf -o /tmp/seg.ts http://127.0.0.1:4190/session/%s/%s && test $(stat -c %%s /tmp/seg.ts) -gt 1000" % (sid, seg))
# a key sealed to some other box is refused
box.fail("curl -sf -X POST -H 'content-type: application/json' -d '{\"chunks\":[\"http://127.0.0.1:8000/00000000\"],\"key\":\"AAAA\",\"size\":1}' http://127.0.0.1:4190/session")
# over: the session and its files are gone
box.succeed("curl -sf -X DELETE http://127.0.0.1:4190/session/%s" % sid)
box.fail("curl -sf http://127.0.0.1:4190/session/%s/index.m3u8" % sid)
box.fail("test -d /run/dd-transcode/%s" % sid)
