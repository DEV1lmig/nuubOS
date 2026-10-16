#!/usr/bin/env python3
"""Third-party notices for the Rust crates and Go modules built into nuubOS.

    third-party-notices.py cargo METADATA_JSON TITLE OUT
    third-party-notices.py go VENDOR_DIR TITLE OUT

cargo: METADATA_JSON is `cargo metadata --format-version 1 --filter-platform
<target>` of the package; only the crates reached from the workspace through
normal (not dev, not build) dependencies are listed: the ones linked into the
program. go: VENDOR_DIR is the module's `vendor/` (modules.txt).

For each crate/module: name, version, declared license, and the license
and notice files it ships (LICENSE*, LICENCE*, COPYING*, NOTICE*, UNLICENSE,
COPYRIGHT*). Identical texts are printed once and referenced by number.
Exits 1 when a component has neither a declared license nor a license file,
so a new dependency without licensing information stops the build.
"""
import hashlib
import json
import os
import re
import sys

LICENSE_FILE = re.compile(r"^(licen[cs]e|copying|notice|unlicense|copyright)([-._].*)?$", re.I)


def license_files(directory):
    try:
        names = sorted(os.listdir(directory))
    except OSError:
        return []
    return [os.path.join(directory, n) for n in names
            if LICENSE_FILE.match(n) and os.path.isfile(os.path.join(directory, n))]


def cargo_components(path):
    with open(path, encoding="utf-8") as f:
        meta = json.load(f)
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    roots = set(meta["workspace_members"])
    seen, todo = set(), list(roots)
    while todo:
        node = nodes[todo.pop()]
        for dep in node["deps"]:
            kinds = dep.get("dep_kinds") or [{"kind": None}]
            if any(k["kind"] is None for k in kinds) and dep["pkg"] not in seen:
                seen.add(dep["pkg"])
                todo.append(dep["pkg"])
    out = []
    for pid in seen - roots:
        p = packages[pid]
        directory = os.path.dirname(p["manifest_path"])
        files = license_files(directory)
        if p.get("license_file"):
            extra = os.path.join(directory, p["license_file"])
            if os.path.isfile(extra) and extra not in files:
                files.append(extra)
        out.append((p["name"], p["version"], p.get("license") or "", files))
    return out


def go_components(vendor):
    out = []
    with open(os.path.join(vendor, "modules.txt"), encoding="utf-8") as f:
        lines = f.read().splitlines()
    for i, line in enumerate(lines):
        if not line.startswith("# ") or line.startswith("# =>"):
            continue
        parts = line[2:].split()
        if len(parts) < 2:
            continue
        # A module with no package line after it contributes no code.
        rest = lines[i + 1:]
        used = False
        for nxt in rest:
            if nxt.startswith("# "):
                break
            if nxt and not nxt.startswith("#"):
                used = True
                break
        if not used:
            continue
        module, version = parts[0], parts[1]
        if "=>" in parts:
            target = parts[parts.index("=>") + 1:]
            if target and target[0].startswith(("./", "../")):
                continue
        files = license_files(os.path.join(vendor, module))
        # Sub-modules (e.g. golang.org/x/net/...) inherit the license at the root.
        parent = module
        while not files and "/" in parent:
            parent = parent.rsplit("/", 1)[0]
            files = license_files(os.path.join(vendor, parent))
        out.append((module, version, "", files))
    return out


def main():
    if len(sys.argv) != 5 or sys.argv[1] not in ("cargo", "go"):
        sys.stderr.write(__doc__)
        return 2
    mode, source, title, dest = sys.argv[1:]
    comps = cargo_components(source) if mode == "cargo" else go_components(source)
    comps.sort(key=lambda c: (c[0].lower(), c[1]))
    texts, order, missing = {}, [], []
    entries = []
    for name, version, lic, files in comps:
        refs = []
        for path in files:
            with open(path, encoding="utf-8", errors="replace") as f:
                text = f.read().strip()
            key = hashlib.sha256(text.encode()).hexdigest()
            if key not in texts:
                texts[key] = (len(order) + 1, text)
                order.append(key)
            refs.append("[%d] %s" % (texts[key][0], os.path.basename(path)))
        if not lic and not refs:
            missing.append("%s %s" % (name, version))
        entries.append((name, version, lic, refs))
    if missing:
        sys.stderr.write("[FAIL] no license information: %s\n" % ", ".join(missing))
        return 1
    with open(dest, "w", encoding="utf-8") as f:
        f.write("%s: third-party components\n\n" % title)
        f.write("%d %s built into this program, with the license each one\n"
                "declares and the license texts it ships (numbered below).\n\n"
                % (len(entries), "crates" if mode == "cargo" else "Go modules"))
        for name, version, lic, refs in entries:
            f.write("%s %s\n" % (name, version))
            if lic:
                f.write("  License: %s\n" % lic)
            for ref in refs:
                f.write("  Text: %s\n" % ref)
            if not refs:
                f.write("  Text: the standard text of the license named above\n")
        f.write("\n")
        for key in order:
            n, text = texts[key]
            f.write("=" * 78 + "\n[%d]\n\n%s\n\n" % (n, text))
    return 0


if __name__ == "__main__":
    sys.exit(main())
