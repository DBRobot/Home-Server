# What a device does before asking a box to transcode: the file in 4 MiB
# chunks, each sealed with the file key at its index (XChaCha20-Poly1305,
# nonce = index, aad = index), and the file key sealed to the box's
# ephemeral x25519 key (a libsodium sealed box). Prints the session body.
import base64, json, os, struct, sys
from nacl.bindings import crypto_aead_xchacha20poly1305_ietf_encrypt
from nacl.public import PublicKey, SealedBox

src, out, box_key_b64, base_url = sys.argv[1:5]
CHUNK = 4 * 1024 * 1024
key = os.urandom(32)
data = open(src, "rb").read()
os.makedirs(out, exist_ok=True)
urls = []
for n in range(0, max(1, (len(data) + CHUNK - 1) // CHUNK)):
    plain = data[n * CHUNK:(n + 1) * CHUNK]
    nonce = struct.pack("<Q", n) + b"\0" * 16
    sealed = crypto_aead_xchacha20poly1305_ietf_encrypt(plain, struct.pack("<Q", n), nonce, key)
    open(os.path.join(out, "%08d" % n), "wb").write(sealed)
    urls.append("%s/%08d" % (base_url, n))
b64 = lambda b: base64.urlsafe_b64encode(b).rstrip(b"=").decode()
pad = lambda s: s + "=" * (-len(s) % 4)
box_pk = PublicKey(base64.urlsafe_b64decode(pad(box_key_b64)))
sealed_key = SealedBox(box_pk).encrypt(key)
print(json.dumps({"chunks": urls, "key": b64(sealed_key), "size": len(data)}))
