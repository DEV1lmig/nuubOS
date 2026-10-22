#!/usr/bin/env python3
# Host test: with a catalog, libraryd's start scan waits for nuubUI ready
# (RUN_ROOT/ui-ready); without one (first boot) it scans at once.
# Run: python3 package/nuubos/nuubos-library/tests/boot-scan.py
import os, socket, subprocess, sys, tempfile, time

here = os.path.dirname(os.path.abspath(__file__))
src = os.path.join(here, '..', 'src')
t = tempfile.mkdtemp()
run, ud, share = t + '/run', t + '/userdata', t + '/share'
user = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'
for d in (run + '/user', ud + '/roms/snes', share, ud + '/users/' + user + '/library'):
    os.makedirs(d, exist_ok=True)
open(share + '/systems.conf', 'w').write(open(src + '/systems.conf').read())
open(run + '/user/active', 'w').write(user + '\n')
open(ud + '/roms/snes/Zelda.sfc', 'w').write('z')

exe = t + '/libraryd'
subprocess.check_call(['gcc', '-std=c11', '-D_GNU_SOURCE', '-Wall', '-Wextra', '-Werror',
    '-DRUN_ROOT="%s"' % run, '-DUSERDATA_ROOT="%s"' % ud, '-DSHARE_ROOT="%s"' % share, '-DSTATE_ROOT="%s"' % (t + '/state'),
    src + '/libraryd.c', '-o', exe, '-lpthread'])

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
        if data.decode().endswith('end=1\n'):
            break
    s.close()
    return data.decode()

def wait_games(n, seconds):
    for _ in range(int(seconds * 10)):
        st = cmd('STATUS')
        if 'scanning=0' in st and ('games=%d\n' % n) in st:
            return True
        time.sleep(0.1)
    return False

fails = 0
def check(name, cond):
    global fails
    if not cond:
        print('[FAIL]', name); fails += 1

# First boot: no catalog, the scan runs without nuubUI.
proc = subprocess.Popen([exe], stderr=open(t + '/log', 'a'))
check('first boot scans at once', wait_games(1, 5))
proc.terminate(); proc.wait()

# Next boot: a new game, nuubUI not ready yet.
open(ud + '/roms/snes/Metroid.sfc', 'w').write('m')
proc = subprocess.Popen([exe], stderr=open(t + '/log', 'a'))
check('catalog served before the scan', wait_games(1, 5))
time.sleep(1)
check('no scan before ui-ready', wait_games(1, 0.2))
open(run + '/ui-ready', 'w').write('ready\n')
check('scan after ui-ready', wait_games(2, 5))
proc.terminate(); proc.wait()

print('boot-scan: %s' % ('FAIL' if fails else 'PASS'))
sys.exit(1 if fails else 0)
