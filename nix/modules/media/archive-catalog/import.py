"""Mirror archive.org's public-domain film catalogue as jellyfin .strm files.

Nothing is downloaded. A .strm holds a URL and jellyfin streams it straight
from archive.org, so this host never carries the bytes and never distributes
anything. If an item is taken down upstream the link simply dies with it.
"""

import json
import os
import re
import sys
import time
import urllib.parse
import urllib.request

SEARCH = "https://archive.org/advancedsearch.php"
METADATA = "https://archive.org/metadata/"
DOWNLOAD = "https://archive.org/download/"

# Curated collections only. Any uploader can set licenseurl on their own item,
# so trusting that field alone is exactly how a bogus "public domain" upload
# would end up in the catalogue. Collection membership is editorial; requiring
# both is the point.
COLLECTIONS = ["feature_films", "publicmovies212", "prelinger"]

# Leading wildcards are rejected, hence the http prefix.
LICENSES = ["http*publicdomain*", "http*creativecommons*"]

UA = "node1-archive-catalog (+https://github.com/DBRobot/Home-Server)"
PREFERRED = [".mp4", ".mkv", ".ogv", ".avi"]


def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


def search(rows, pages):
    q = (
        "collection:(" + " OR ".join(COLLECTIONS) + ")"
        " AND mediatype:movies"
        " AND licenseurl:(" + " OR ".join(LICENSES) + ")"
    )
    found = {}
    for page in range(1, pages + 1):
        params = urllib.parse.urlencode(
            {
                "q": q,
                "rows": rows,
                "page": page,
                "output": "json",
                "sort[]": "downloads desc",
            },
            doseq=True,
        )
        params += "".join(
            "&fl[]=" + f for f in ("identifier", "title", "year", "licenseurl")
        )
        docs = get(SEARCH + "?" + params)["response"]["docs"]
        if not docs:
            break
        for d in docs:
            if d.get("identifier"):
                found[d["identifier"]] = d
        time.sleep(1)
    return found


def safe(name):
    name = re.sub(r"[/\\:*?\"<>|]", "-", str(name)).strip()
    name = re.sub(r"\s+", " ", name)
    return name[:150] or "untitled"


def pick_file(identifier):
    """Largest file with a preferred extension; None if the item has no video."""
    try:
        meta = get(METADATA + identifier)
    except Exception as e:
        print(f"  metadata failed for {identifier}: {e}", file=sys.stderr)
        return None
    best, best_rank, best_size = None, len(PREFERRED), -1
    for f in meta.get("files", []):
        name = f.get("name", "")
        ext = os.path.splitext(name)[1].lower()
        if ext not in PREFERRED:
            continue
        rank = PREFERRED.index(ext)
        try:
            size = int(f.get("size", 0))
        except (TypeError, ValueError):
            size = 0
        if rank < best_rank or (rank == best_rank and size > best_size):
            best, best_rank, best_size = name, rank, size
    return best


def main():
    target = os.environ.get("CATALOG_DIR", "/srv/catalog/public")
    rows = int(os.environ.get("CATALOG_ROWS", "100"))
    pages = int(os.environ.get("CATALOG_PAGES", "3"))
    os.makedirs(target, exist_ok=True)

    wanted = search(rows, pages)
    print(f"search matched {len(wanted)} items")
    if not wanted:
        print("no results - refusing to prune against an empty search",
              file=sys.stderr)
        return 1

    # Map the .strm files already on disk back to their identifiers, so a
    # rerun only fetches metadata for genuinely new items.
    have = {}
    for entry in os.listdir(target):
        if not entry.endswith(".strm"):
            continue
        path = os.path.join(target, entry)
        try:
            with open(path) as fh:
                url = fh.read().strip()
        except OSError:
            continue
        if url.startswith(DOWNLOAD):
            have[url[len(DOWNLOAD):].split("/")[0]] = path

    added = 0
    for ident, doc in wanted.items():
        if ident in have:
            continue
        name = pick_file(ident)
        time.sleep(0.3)  # upstream is a donation-funded archive; be polite
        if not name:
            continue
        year = str(doc.get("year", "")).strip()
        title = safe(doc.get("title", ident))
        stem = f"{title} ({year})" if year.isdigit() else title
        url = DOWNLOAD + ident + "/" + urllib.parse.quote(name)
        with open(os.path.join(target, stem + ".strm"), "w") as fh:
            fh.write(url + "\n")
        added += 1

    # Anything absent from this run's results goes away: taken down upstream,
    # licence changed, or simply pushed out of the top N by download count.
    # The catalogue is "the top N", not "everything ever matched", so some
    # churn at the boundary is expected rather than a fault.
    removed = 0
    for ident, path in have.items():
        if ident not in wanted:
            os.remove(path)
            removed += 1

    print(f"added {added}, removed {removed}, total {len(os.listdir(target))}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
