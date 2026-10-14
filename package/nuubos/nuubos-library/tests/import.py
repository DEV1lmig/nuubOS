#!/usr/bin/env python3
# Host test: nuubos-library-import (plan + run), the same placement as the
# Web administration's Add Games.
# Run: python3 package/nuubos/nuubos-library/tests/import.py
import os, subprocess, sys, tempfile

here = os.path.dirname(os.path.abspath(__file__))
src = os.path.join(here, '..', 'src')
t = tempfile.mkdtemp()
exe = t + '/library-import'
subprocess.check_call(['gcc', '-std=c11', '-D_GNU_SOURCE', '-Wall', '-Wextra', '-Werror',
    '-Wno-format-truncation', '-DREGISTRY_PATH="%s"' % (src + '/systems.conf'),
    '-DLIBRARY_SOCKET="%s"' % (t + '/none.sock'), src + '/library-import.c', '-o', exe])
fails = 0

def check(name, cond):
    global fails
    if not cond:
        print('[FAIL]', name)
        fails += 1

def w(path, data='x'):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    open(path, 'wb').write(data.encode() if isinstance(data, str) else data)

def plan(path):
    out = subprocess.run([exe, 'plan', path], capture_output=True, text=True).stdout
    head = dict(kv.split('=') for kv in out.splitlines()[0].split())
    place = {l.split('\t')[1]: int(l.split('\t')[2]) for l in out.splitlines() if l.startswith('place\t')}
    asks = [l.split('\t')[1:] for l in out.splitlines() if l.startswith('ask\t')]
    return head, place, asks

def run(path, answers='', root=None):
    env = dict(os.environ, NUUBOS_GAMES_ROOT=root or t + '/roms')
    return subprocess.run([exe, 'run', path, answers], capture_output=True, text=True, env=env).stdout

usb = t + '/usb'
# PS1 multi-disc: .m3u + .chd, loose in a folder (CHD tells no system).
w(usb + '/FF7/FF7.m3u', 'FF7 (Disc 1).chd\nFF7 (Disc 2).chd\r\nFF7 (Disc 3).chd\n')
for d in (1, 2, 3):
    w(usb + '/FF7/FF7 (Disc %d).chd' % d, 'c' * 10)
head, place, asks = plan(usb + '/FF7')
check('m3u+chd: one question for the whole set', len(asks) == 1 and asks[0][2] == '4' and 'psx' in asks[0][3].split(','))
check('m3u+chd: nothing placed before the answer', place == {} and head['files'] == '4')
cands = [l.split('\t') for l in subprocess.run([exe, 'plan', usb + '/FF7'], capture_output=True, text=True).stdout.splitlines() if l.startswith('cand\t')]
check('candidates carry the display name', ['cand', '0', 'psx', 'PlayStation'] in cands and len(cands) == len(asks[0][3].split(',')))
out = run(usb + '/FF7', '0=psx')
check('m3u+chd: copied to psx keeping the folder', '@result 4 0 0' in out and
      os.path.isfile(t + '/roms/psx/FF7/FF7.m3u') and os.path.isfile(t + '/roms/psx/FF7/FF7 (Disc 3).chd'))
check('no .part left', not any(n.endswith('.part') for _, _, fs in os.walk(t + '/roms') for n in fs))
out = run(usb + '/FF7', '0=psx')
check('second import: already there, nothing overwritten', '@result 0 4 0' in out)
out = run(usb + '/FF7', '', t + '/roms2')
check('unanswered question: skipped', '@result 0 0 4' in out and not os.path.exists(t + '/roms2/psx'))

# m3u -> cue -> bin in a subfolder, PS1 header in the first track.
w(usb + '/MGS/MGS.m3u', '#EXTM3U\ndiscs/MGS (Disc 1).cue\ndiscs\\MGS (Disc 2).cue|Disc 2\n')
w(usb + '/MGS/discs/MGS (Disc 1).cue', 'FILE "MGS (Disc 1).bin" BINARY\n  TRACK 01 MODE2/2352\n')
w(usb + '/MGS/discs/MGS (Disc 1).bin', b'\0' * 37664 + b'PLAYSTATION' + b'\0' * 100)
w(usb + '/MGS/discs/MGS (Disc 2).cue', 'FILE "MGS (Disc 2).bin" BINARY\n')
w(usb + '/MGS/discs/MGS (Disc 2).bin', b'\0' * 100)
head, place, asks = plan(usb + '/MGS')
check('nested set, PS1 header: no question', asks == [] and place == {'psx': 5})

# .m3u + .pbp: only psx takes both.
w(usb + '/Mixed/Game.m3u', 'Game CD1.pbp\nGame CD2.pbp\n')
w(usb + '/Mixed/Game CD1.pbp'); w(usb + '/Mixed/Game CD2.pbp')
# Floppy set and a cartridge in the same folder, plus a readme.
w(usb + '/Mixed/Turrican.m3u', 't1.d64\nt2.d64\n')
w(usb + '/Mixed/t1.d64'); w(usb + '/Mixed/t2.d64')
w(usb + '/Mixed/zelda.gba')
w(usb + '/Mixed/readme.txt')
w(usb + '/Mixed/.DS_Store')
head, place, asks = plan(usb + '/Mixed')
check('pbp set placed on psx, gba placed', place.get('psx') == 3 and place.get('gba') == 1)
check('floppy set asked once among the Commodore systems',
      len(asks) == 1 and asks[0][1] == 'Turrican.m3u' and 'c64' in asks[0][3].split(',') and 'psx' not in asks[0][3].split(','))
check('readme skipped, dot file ignored', head['skipped'] == '1' and head['files'] == '7')

# A folder named like a system decides, the path below it is kept.
w(usb + '/roms/PlayStation/FF8/FF8.m3u', 'FF8 1.chd\n')
w(usb + '/roms/PlayStation/FF8/FF8 1.chd')
w(usb + '/roms/snes/Mario.sfc')
head, place, asks = plan(usb + '/roms')
check('system folders (name and id) decide', asks == [] and place == {'psx': 2, 'snes': 1})
out = run(usb + '/roms', '', t + '/roms3')
check('system folder layout kept', os.path.isfile(t + '/roms3/psx/FF8/FF8.m3u') and os.path.isfile(t + '/roms3/snes/Mario.sfc'))

# cue with an extra track it does not name; Sega CD header.
w(usb + '/X/X.cue', 'FILE "X (Track 1).bin" BINARY\n')
w(usb + '/X/X (Track 1).bin', 'SEGADISCSYSTEM  ')
w(usb + '/X/X (Track 2).bin')
head, place, asks = plan(usb + '/X')
check('unnamed track follows its folder index, Sega CD sniffed', asks == [] and place == {'segacd': 3})

# gdi
w(usb + '/DC/game.gdi', '3\n1 0 4 2352 track01.bin 0\n2 756 0 2352 "track 02.raw" 0\n')
w(usb + '/DC/track01.bin'); w(usb + '/DC/track 02.raw')
head, place, asks = plan(usb + '/DC')
check('gdi set on dreamcast', asks == [] and place == {'dreamcast': 3})

# A single file.
head, place, asks = plan(usb + '/Mixed/zelda.gba')
check('single file', place == {'gba': 1} and head['files'] == '1')
out = run(usb + '/Mixed/zelda.gba')
check('single file copied at the system root', os.path.isfile(t + '/roms/gba/zelda.gba'))

# Errors
check('missing source', '@error source' in run(t + '/nope'))

print('[PASS] library-import' if not fails else '[FAIL] library-import')
sys.exit(1 if fails else 0)
