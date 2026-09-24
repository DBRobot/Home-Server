# What a device does before asking a box to transcode, with the file
# already in rclone's crypt format (rclone wrote it): the sealed file goes
# where the box can fetch ranges of it, and the library's data key (the
# first 32 bytes rclone derives with scrypt from password and salt) is
# sealed to the box's ephemeral x25519 key, a libsodium sealed box.
# Prints the session body.
import base64, hashlib, json, os, sys
from nacl.public import PublicKey, SealedBox

sealed_path, password, salt, box_key_b64, url = sys.argv[1:6]
key = hashlib.scrypt(password.encode(), salt=salt.encode(), n=16384, r=8, p=1, dklen=80)[:32]
b64 = lambda b: base64.urlsafe_b64encode(b).rstrip(b"=").decode()
pad = lambda s: s + "=" * (-len(s) % 4)
box_pk = PublicKey(base64.urlsafe_b64decode(pad(box_key_b64)))
print(json.dumps({"url": url, "key": b64(SealedBox(box_pk).encrypt(key)), "size": os.path.getsize(sealed_path)}))
