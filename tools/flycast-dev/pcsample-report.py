#!/usr/bin/env python3
# pcsample-report.py MAPS PCS [ROOT]: top functions of a pcsample run.
# MAPS = copy of /proc/PID/maps, PCS = pcsample output ("pc lr x1" lines),
# ROOT = where the unstripped files live (default output/h700/target; link
# the build-dir core and the sysroot libc there for full symbols). Uses the
# toolchain nm. Samples in libc are also listed by their caller (the link
# register). A sleeping thread is sampled too: its time shows in the
# syscall wrappers.
import bisect, collections, glob, os, subprocess, sys

maps, pcs = sys.argv[1], sys.argv[2]
root = sys.argv[3] if len(sys.argv) > 3 else 'output/h700/target'
nm = glob.glob('output/h700/host/bin/aarch64-buildroot-linux-gnu-nm')[0]

regions = []
for line in open(maps):
    f = line.split()
    if len(f) < 6 or 'x' not in f[1]:
        continue
    lo, hi = (int(x, 16) for x in f[0].split('-'))
    regions.append((lo, hi, int(f[2], 16), f[5]))

syms = {}
def symbols(path):
    if path not in syms:
        tab = []
        local = root + path
        if os.path.exists(local):
            out = subprocess.run([nm, '-C', '--defined-only', local], capture_output=True, text=True).stdout
            if not out.strip():
                out = subprocess.run([nm, '-C', '-D', '--defined-only', local], capture_output=True, text=True).stdout
            for l in out.splitlines():
                p = l.split(' ', 2)
                if len(p) == 3 and p[1] in 'tTwW':
                    tab.append((int(p[0], 16), p[2]))
        tab.sort()
        syms[path] = tab
    return syms[path]

def resolve(pc):
    for lo, hi, off, path in regions:
        if lo <= pc < hi:
            tab = symbols(path)
            # shared objects: file offset == link address for the text segment
            i = bisect.bisect_right(tab, (pc - lo + off, chr(0x10ffff))) - 1
            return (tab[i][1] if i >= 0 else '?') + '  [' + os.path.basename(path) + ']'
    return '[unknown/JIT]'

count = collections.Counter()
callers = collections.Counter()
ioctls = collections.Counter()
total = 0
for l in open(pcs):
    f = l.split()
    total += 1
    name = resolve(int(f[0], 16))
    count[name] += 1
    if '[libc.so.6]' in name and len(f) > 1:
        callers[name.split()[0] + '  <-  ' + resolve(int(f[1], 16))] += 1
    if name.startswith('ioctl ') and len(f) > 2:
        ioctls[f[2]] += 1
for name, c in count.most_common(40):
    print(f'{100 * c / total:5.1f}%  {name}')
print(f'{total} samples')
if callers:
    print('libc by caller:')
    for name, c in callers.most_common(15):
        print(f'{100 * c / total:5.1f}%  {name}')
if ioctls:
    print('ioctl requests (DRM: 0x64 type, low byte = number + 0x40 driver base):')
    for req, c in ioctls.most_common(8):
        print(f'{100 * c / total:5.1f}%  0x{req}')
