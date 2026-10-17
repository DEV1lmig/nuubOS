import sys, subprocess, bisect, collections
samples, maps, root = sys.argv[1], sys.argv[2], sys.argv[3:]
NM = 'output/h700/host/bin/aarch64-buildroot-linux-gnu-nm'
segs = []
for l in open(maps):
    p = l.split()
    if len(p) >= 6 and 'x' in p[1]:
        a, b = (int(x, 16) for x in p[0].split('-'))
        segs.append((a, b, int(p[2], 16), p[5]))
def find_local(path):
    import os, glob
    base = os.path.basename(path)
    for r in root:
        for c in glob.glob(r + '/**/' + base, recursive=True):
            return c
    return None
symcache = {}
def syms(dso):
    if dso in symcache: return symcache[dso]
    f = find_local(dso); tab = []
    if f:
        out = subprocess.run([NM, '-C', '--defined-only', '-n', f], capture_output=True, text=True).stdout
        if not out.strip():
            out = subprocess.run([NM, '-C', '-D', '--defined-only', '-n', f], capture_output=True, text=True).stdout
        for l in out.splitlines():
            p = l.split(' ', 2)
            if len(p) == 3 and p[1].lower() in 'tw':
                tab.append((int(p[0], 16), p[2]))
    symcache[dso] = ([a for a, _ in tab], [n for _, n in tab], f)
    return symcache[dso]
per = collections.defaultdict(collections.Counter); tot = collections.Counter()
for l in open(samples):
    p = l.split()
    if len(p) < 2: continue
    tid, ip = p[0], int(p[1], 16)
    name = None
    for a, b, off, path in segs:
        if a <= ip < b:
            addrs, names, f = syms(path)
            rel = ip - a + off
            i = bisect.bisect_right(addrs, rel) - 1
            name = (path.split('/')[-1] + ':' + (names[i] if i >= 0 else hex(rel)))
            break
    if name is None: name = '[anon/JIT]'
    per[tid][name] += 1; tot[tid] += 1
for tid, c in sorted(tot.items(), key=lambda x: -x[1]):
    print(f'== tid {tid}: {c} samples')
    for n, k in per[tid].most_common(25):
        print(f'  {100*k/c:5.1f}%  {n[:150]}')
