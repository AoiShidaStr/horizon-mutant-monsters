#!/usr/bin/env python3
"""Pack this mod's two files the way docs/api/mod-files.md describes them.

  mutant_monsters.hmod   horizon-mod.toml + assets/ + data/
  mutant_monsters.hnmod  horizon-native.json + mixins/mutant_monsters/...
                         (the Rust source, with LICENSE, NOTICE.md and
                         README.md: AGPL-3.0 needs the licence text and the
                         corresponding source next to the code)

Run: python tools/pack.py [--spawns]

--spawns adds optional/spawns/data/ to the .hmod: the three mutants then spawn
naturally in the overworld (weight 5, about 5% of zombie spawns, upstream's
default). Off by default until the mob has been tried on the server.
"""
import hashlib
import json
import os
import sys
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ID = "mutant_monsters"
HORIZON = "a85af177a82504f357ff10d5d40d0b7230e2ac32"
# launcher/src/native_mods.rs MIXINS_REV at that Horizon commit.
MIXINS = "5a64af938ec86b00d53d16a408aabc0038fb3e57"
GAME = "26.3"
# NATIVE_HOOKS_API of the pinned Horizon commit (crates/horizon-client/src/game/native_hooks.rs).
HOOKS = 415

BUILD = os.path.join(ROOT, "target")


def manifest_version():
    with open(os.path.join(ROOT, "content", "horizon-mod.toml"), encoding="utf-8") as f:
        for line in f:
            if line.strip().startswith("version"):
                return line.split("=", 1)[1].strip().strip('"')
    raise SystemExit("no version in content/horizon-mod.toml")


def add_tree(zf, base, prefix="", skip=()):
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = sorted(d for d in dirnames if d not in skip and not d.startswith("."))
        for name in sorted(filenames):
            if name.startswith("."):
                continue
            full = os.path.join(dirpath, name)
            rel = os.path.relpath(full, base).replace("\\", "/")
            zf.write(full, (prefix + rel) if prefix else rel)


def build_content(spawns):
    out = os.path.join(BUILD, ID + ".hmod")
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as zf:
        zf.write(os.path.join(ROOT, "content", "horizon-mod.toml"), "horizon-mod.toml")
        add_tree(zf, os.path.join(ROOT, "content", "assets"), "assets/")
        add_tree(zf, os.path.join(ROOT, "content", "data"), "data/")
        if spawns:
            add_tree(zf, os.path.join(ROOT, "optional", "spawns", "data"), "data/")
    return out


def build_native():
    out = os.path.join(BUILD, ID + ".hnmod")
    bundle = os.path.join(ROOT, "mixins", ID)
    native = {
        "schema": 1,
        "id": ID,
        "version": manifest_version(),
        "game": GAME,
        "horizon": HORIZON,
        "mixins": MIXINS,
        "hooks": HOOKS,
    }
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as zf:
        zf.writestr("horizon-native.json", json.dumps(native, indent=2) + "\n")
        add_tree(zf, bundle, "mixins/" + ID + "/", skip=("target", ".build"))
        for name in ("LICENSE", "NOTICE.md", "README.md"):
            zf.write(os.path.join(ROOT, name), "mixins/" + ID + "/" + name)
    return out


def main():
    spawns = "--spawns" in sys.argv[1:]
    os.makedirs(BUILD, exist_ok=True)
    for path in (build_content(spawns), build_native()):
        with open(path, "rb") as f:
            digest = hashlib.sha256(f.read()).hexdigest()
        print(f"{os.path.relpath(path, ROOT)}  {os.path.getsize(path)} bytes  sha256 {digest}")


if __name__ == "__main__":
    main()
