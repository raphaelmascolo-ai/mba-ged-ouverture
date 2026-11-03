#!/bin/bash
# Compose latest.json (updater Tauri) depuis les signatures d'une release : usage scripts-latest-json.sh vX.Y.Z
set -e
TAG="$1"; REPO="raphaelmascolo-ai/mba-ged-ouverture"; D=$(mktemp -d)
gh release download "$TAG" --repo "$REPO" --pattern "*.sig" --dir "$D"
python3 - "$TAG" "$D" <<'PY'
import json, os, sys, datetime, urllib.parse
tag, d = sys.argv[1], sys.argv[2]
base = f"https://github.com/raphaelmascolo-ai/mba-ged-ouverture/releases/download/{tag}/"
plats = {}
for f in os.listdir(d):
    sig = open(os.path.join(d, f)).read().strip(); asset = f[:-4]
    url = base + urllib.parse.quote(asset)
    if asset.endswith(".app.tar.gz"):
        for k in ["darwin-universal", "darwin-aarch64", "darwin-x86_64"]: plats[k] = {"signature": sig, "url": url}
    elif asset.endswith(".msi"):
        plats["windows-x86_64"] = {"signature": sig, "url": url}
json.dump({"version": tag.lstrip("v"), "notes": "MBA GED – Ouverture", "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), "platforms": plats}, open(os.path.join(d, "latest.json"), "w"), indent=1)
print("plateformes :", ", ".join(sorted(plats)))
PY
gh release upload "$TAG" --repo "$REPO" --clobber "$D/latest.json"
