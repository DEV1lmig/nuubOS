#!/usr/bin/env python3
# Host test: PortMaster ports (EPIC-058) in the library: the scripts directly
# in roms/ports, never a port's own folder, another folder or SD2; cover,
# year, genre and details from PortMaster's gameinfo.xml.
# Run: python3 package/nuubos/nuubos-library/tests/ports.py
import os, socket, subprocess, sys, tempfile, time

here = os.path.dirname(os.path.abspath(__file__))
src = os.path.join(here, '..', 'src')
t = tempfile.mkdtemp()
run, ud, share = t + '/run', t + '/userdata', t + '/share'
user = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'
ports = ud + '/roms/ports'
for d in (run + '/user', share, ports + '/ametadatagame/gamedata', ports + '/PortMaster',
          ports + '/.nuubos/home', ud + '/roms/snes', ports + '/bare'):
    os.makedirs(d, exist_ok=True)
open(share + '/systems.conf', 'w').write(open(src + '/systems.conf').read())
open(run + '/user/active', 'w').write(user + '\n')
def w(p, data): open(ports + '/' + p, 'w').write(data)
w('A Meta Data Game.sh', '#!/bin/bash\n')
w('Bare Port.sh', '#!/bin/bash\n')
w('ametadatagame/gameinfo.xml', '''<?xml version="1.0" encoding="utf-8"?>
<gameList>
  <game>
    <path>./A Meta Data Game.sh</path>
    <name>A Meta Data Game</name>
    <desc>Embark on a journey.
Solve puzzles &amp; more.</desc>
    <releasedate>20200423T000000</releasedate>
    <developer>Luis Zuno (aka Ansimuz)</developer>
    <genre>Adventure-puzzle</genre>
    <image>./ametadatagame/screenshot.png</image>
  </game>
</gameList>
''')
w('ametadatagame/screenshot.png', 'png')
# A port's data: archives and scripts that look like games, never listed.
w('ametadatagame/gamedata/level.zip', 'zip')
w('ametadatagame/gamedata/run.sh', '#!/bin/bash\n')
w('PortMaster/PortMaster.sh', '#!/bin/bash\n')
w('.nuubos/home/x.sh', '#!/bin/bash\n')
open(ud + '/roms/snes/tool.sh', 'w').write('#!/bin/bash\n')

exe = t + '/libraryd'
subprocess.check_call(['gcc', '-std=c11', '-D_GNU_SOURCE', '-Wall', '-Wextra', '-Werror',
    '-DRUN_ROOT="%s"' % run, '-DUSERDATA_ROOT="%s"' % ud, '-DSHARE_ROOT="%s"' % share,
    '-DSTATE_ROOT="%s"' % (t + '/state'), src + '/libraryd.c', '-o', exe, '-lpthread'])
proc = subprocess.Popen([exe], stderr=open(t + '/log', 'w'))
fails = 0
def check(name, cond):
    global fails
    if not cond:
        print('[FAIL]', name); fails += 1

def cmd(line):
    s = socket.socket(socket.AF_UNIX)
    for _ in range(50):
        try:
            s.connect(run + '/libraryd.sock'); break
        except OSError:
            time.sleep(0.1)
    s.sendall((line + '\n').encode())
    data = b''
    s.settimeout(2)
    while True:
        try:
            chunk = s.recv(65536)
        except socket.timeout:
            break
        if not chunk: break
        data += chunk
        txt = data.decode()
        if txt.endswith('end=1\n') or (not txt.startswith(('game=', 'user=')) and txt.endswith('\n')):
            break
    s.close()
    return data.decode()

try:
    for _ in range(50):
        st = cmd('STATUS')
        if 'scanning=0' in st and 'games=' in st and 'games=0' not in st: break
        time.sleep(0.1)
    rows = [l[5:].split('\t') for l in cmd('GAMES\tsystem:ports').splitlines() if l.startswith('game=')]
    titles = sorted(r[2] for r in rows)
    check('only the two scripts of roms/ports', titles == ['A Meta Data Game', 'Bare Port'])
    meta = next((r for r in rows if r[2] == 'A Meta Data Game'), None)
    check('cover from gameinfo.xml', meta and meta[3] == ports + '/ametadatagame/screenshot.png')
    check('year and genre from gameinfo.xml', meta and meta[9] == '2020' and meta[11] == 'Adventure-puzzle')
    bare = next((r for r in rows if r[2] == 'Bare Port'), None)
    check('a port without gameinfo.xml', bare and bare[3] == '')
    st = cmd('STATUS'); check('no data file or script elsewhere listed', '\ngames=2\n' in '\n' + st)
    d = cmd('DETAILS\t' + meta[0])
    check('details description on one line', 'meta_description=Embark on a journey. Solve puzzles & more.\n' in d)
    check('details developer and release', 'meta_developer=Luis Zuno (aka Ansimuz)\n' in d and 'meta_release=2020\n' in d)
    check('negative control: STATUS has a games count', 'games=' in st)
finally:
    proc.terminate(); proc.wait()
if fails:
    print(open(t + '/log').read())
    sys.exit(1)
print('[PASS] ports')
