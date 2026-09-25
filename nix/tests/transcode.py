# The transcode test: a box plays a file it cannot read at rest.
import json

box.wait_for_unit("dd-transcode.service")
box.wait_for_open_port(4190)
# A clip big enough that ffmpeg must seek for the index at its end, as a
# real file makes it, and long enough that the transcode of it is still
# running when the test ends the session: twenty seconds encodes faster
# than the test can reach the DELETE, which would make "it was killed"
# indistinguishable from "it had already finished".
box.succeed("ffmpeg -hide_banner -loglevel error -f lavfi -i testsrc=duration=120:size=640x360:rate=25 -f lavfi -i sine=frequency=440:duration=120 -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest /tmp/clip.mp4")
# the box's key for this run, then the clip in the format a library holds it
# (rclone writes it; the data key is what a device would seal to the box)
key = json.loads(box.succeed("curl -sf http://127.0.0.1:4190/key"))["key"]
box.succeed("mkdir -p /tmp/store && printf '[enc]\\ntype = crypt\\nremote = /tmp/store\\npassword = %s\\npassword2 = %s\\n' \"$(rclone obscure 'a library key')\" \"$(rclone obscure 'a996a28ca51c9cf1d3f8e2038c8339c8')\" > /tmp/rclone.conf")
box.succeed("rclone --config /tmp/rclone.conf copyto /tmp/clip.mp4 enc:clip.mp4")
sealed = box.succeed("ls /tmp/store | head -1").strip()
# The bucket honours Range and the box depends on it: a piece is one
# ranged GET, and the block at that offset is decrypted from where the
# range starts. python3 -m http.server does NOT honour Range - it answers
# 200 with the whole file - so the first block came back as the header
# plus block zero and would not open. rclone serves ranges, as garage
# does, which is what this is standing in for.
box.succeed("(rclone serve http --addr 127.0.0.1:8000 /tmp/store >/dev/null 2>&1 &)")
box.wait_until_succeeds("curl -sf -o /dev/null http://127.0.0.1:8000/", timeout=30)
assert "206" == box.succeed("curl -s -o /dev/null -w '%%{http_code}' -H 'range: bytes=0-9' http://127.0.0.1:8000/%s" % sealed).strip(), "the file server must honour Range"
body = box.succeed("python3 %s /tmp/store/%s 'a library key' a996a28ca51c9cf1d3f8e2038c8339c8 '%s' http://127.0.0.1:8000/%s" % (nix["chunker"], sealed, key, sealed)).strip()
box.succeed("printf '%s' '%s' > /tmp/body.json" % ("%s", body.replace("'", "'\\''")))
started = json.loads(box.succeed("curl -sf -X POST -H 'content-type: application/json' -d @/tmp/body.json http://127.0.0.1:4190/session"))
sid = started["id"]
# the playlist grows as ffmpeg works; a segment is playable bytes
box.wait_until_succeeds("curl -sf http://127.0.0.1:4190/session/%s/index.m3u8 | grep -q '\\.ts'" % sid, timeout=120)
seg = box.succeed("curl -sf http://127.0.0.1:4190/session/%s/index.m3u8 | grep '\\.ts' | head -1" % sid).strip()
box.succeed("curl -sf -o /tmp/seg.ts http://127.0.0.1:4190/session/%s/%s && test $(stat -c %%s /tmp/seg.ts) -gt 1000" % (sid, seg))
# a key sealed to some other box is refused
box.fail("curl -sf -X POST -H 'content-type: application/json' -d '{\"url\":\"http://127.0.0.1:8000/x\",\"key\":\"AAAA\",\"size\":100}' http://127.0.0.1:4190/session")
# the work is really happening, and ending the session really ends it:
# a viewer who closes a film after a minute must not leave the box
# transcoding the rest of it
box.succeed("pgrep -f '[f]fmpeg.*%s' >/dev/null" % sid)
box.succeed("curl -sf -X DELETE http://127.0.0.1:4190/session/%s" % sid)
box.wait_until_fails("pgrep -f '[f]fmpeg.*%s' >/dev/null" % sid, timeout=30)
# over: the session and its files are gone
box.fail("curl -sf http://127.0.0.1:4190/session/%s/index.m3u8" % sid)
box.fail("test -d /run/dd-transcode/%s" % sid)
