#!/usr/bin/env python3
# Host test: play history across the user's consoles (Syncthing Play
# Activity): own record exported to library/consoles/<console-id>.tsv, other
# consoles' files added up (time, sessions; latest play for Recently
# Played), a statistics reset that also cancels the other consoles' totals.
# Run: python3 package/nuubos/nuubos-library/tests/history-sync.py
import os, socket, subprocess, sys, tempfile, time

here = os.path.dirname(os.path.abspath(__file__))
src = os.path.join(here, '..', 'src')
t = tempfile.mkdtemp()
run, ud, share = t + '/run', t + '/userdata', t + '/share'
user = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'
for d in (run + '/user', ud + '/roms/psx/ff7', ud + '/roms/snes', share,
          ud + '/library/metadata/snes', ud + '/users/' + user + '/saves/snes'):
    os.makedirs(d, exist_ok=True)
open(share + '/systems.conf', 'w').write(open(src + '/systems.conf').read())
open(run + '/user/active', 'w').write(user + '\n')
def w(p, data): open(ud + '/roms/' + p, 'w').write(data)
w('snes/Mario (USA).sfc', 'x' * 100)
w('snes/Zelda.sfc', 'y' * 200)
w('psx/ff7/FF7 (Disc 1).bin', 'a' * 50)
w('psx/ff7/FF7 (Disc 2).bin', 'b' * 60)
w('psx/ff7/FF7 (Disc 1).cue', 'FILE "FF7 (Disc 1).bin" BINARY\n')
w('psx/ff7/FF7 (Disc 2).cue', 'FILE "FF7 (Disc 2).bin" BINARY\n')
w('psx/ff7/FF7.m3u', 'FF7 (Disc 1).cue\nFF7 (Disc 2).cue\n')
open(ud + '/library/metadata/snes/Mario (USA).txt', 'w').write(
    'title=Super Mario World\ndeveloper=Nintendo\nbad key=x\nplayers=1-2\n')
open(ud + '/users/' + user + '/saves/snes/Mario (USA).srm', 'w').write('save')

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


def field(line, i): return line.split('=', 1)[1].split('\t')[i]
def game_line(gid):
    return next((l for l in cmd('DETAILS\t' + gid).splitlines() if l.startswith('game=')), '')
def recent():
    return [field(l, 0) for l in cmd('GAMES\trecent').splitlines() if l.startswith('game=')]
lib = ud + '/users/' + user + '/library'
def deliver(name, text):
    # As Syncthing does: a temporary dot file, then a rename.
    tmp = lib + '/consoles/.syncthing.' + name + '.tmp'
    open(tmp, 'w').write(text); os.rename(tmp, lib + '/consoles/' + name)
def wait(c, s=4):
    e = time.time() + s
    while time.time() < e:
        if c(): return True
        time.sleep(0.05)
    return False

try:
    for _ in range(50):
        st = cmd('STATUS')
        if 'scanning=0' in st and 'games=' in st and 'games=0' not in st: break
        time.sleep(0.1)
    games = cmd('GAMES\tsystem:snes')
    ids = {l.split('\t')[2]: l.split('=')[1].split('\t')[0] for l in games.splitlines() if l.startswith('game=')}
    mario, zelda = ids.get('Mario'), ids.get('Zelda')
    check('two snes games', mario and zelda)
    cid = open(t + '/state/config/console-id').read().strip()
    check('console id created', len(cid) >= 8)

    cmd('SESSION_RECORD\t%s\t100' % mario)
    own = lib + '/consoles/' + cid + '.tsv'
    check('own record exported', os.path.exists(own) and mario in open(own).read())

    now = int(time.time())
    deliver('other.tsv', '# x\n%s %d 500 2\n%s %d 50 1\n' % (zelda, now + 30, mario, now - 3600))
    check('other console added up', wait(lambda: field(game_line(mario), 6) == '150'))
    check('sessions added up', 'sessions=2' in cmd('DETAILS\t' + mario))
    check('recently played merged', wait(lambda: recent()[:2] == [zelda, mario]))
    deliver('broken.tsv.sync-conflict-20261012-x.tsv', '%s %d 9999 9\n' % (mario, now))
    deliver('.hidden.tsv', '%s %d 9999 9\n' % (mario, now))
    time.sleep(0.3)
    check('conflict copies and dot files ignored', field(game_line(mario), 6) == '150')

    check('reset', cmd('STATS_RESET\t' + mario).startswith('OK'))
    check('reset: zero here', field(game_line(mario), 6) == '0' and 'sessions=0' in cmd('DETAILS\t' + mario))
    check('reset: not recent', mario not in recent())
    check('reset travels in the own record', ' -50 -1 ' in open(own).read())

    # The other console plays Mario after the reset: only that counts.
    deliver('other.tsv', '%s %d 500 2\n%s %d 80 2\n' % (zelda, now + 30, mario, now + 60))
    check('play after reset counts', wait(lambda: field(game_line(mario), 6) == '30'))
    check('recent again', mario in recent())
    check('history.tsv keeps only own games', zelda not in open(lib + '/history.tsv').read())

    # What another console sees from this one's export: reset offsets apply.
    proc.terminate(); proc.wait()
    os.rename(own, lib + '/consoles/first.tsv')
    open(t + '/state/config/console-id', 'w').write('11111111-2222')
    open(lib + '/history.tsv', 'w').write('')
    proc = subprocess.Popen([exe], stderr=open(t + '/log2', 'w'))
    check('seen from the other side', wait(lambda: field(game_line(mario), 6) == '30'))
    # Older history (no export yet) is exported when the user is loaded.
    proc.terminate(); proc.wait()
    open(lib + '/history.tsv', 'w').write('%s %d 70 1\n' % (zelda, now - 99))
    for f in os.listdir(lib + '/consoles'):
        if f.startswith('11111111'): os.unlink(lib + '/consoles/' + f)
    proc = subprocess.Popen([exe], stderr=open(t + '/log3', 'w'))
    check('existing history exported at start', wait(lambda: os.path.exists(lib + '/consoles/11111111-2222.tsv')
          and zelda in open(lib + '/consoles/11111111-2222.tsv').read()))
finally:
    proc.terminate(); proc.wait()
print('[PASS] library history sync' if not fails else '[FAIL] %d checks' % fails)
sys.exit(1 if fails else 0)
