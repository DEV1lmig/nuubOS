#!/usr/bin/env python3
# Host test: nuubos-syncd runs Syncthing only for the active, enabled user
# (switches with the user, no restart storm), applies the nuubOS policy
# (device name, local-only network, no reports, the user's folders shared
# with every accepted device), publishes snapshots to subscribers when
# Syncthing's events say something changed, and raises/dismisses the
# "sync.request" notification. nuubos-syncctl is its command line. Fake
# Syncthing (REST on the unix socket) and fake notifyd.
# Run: python3 package/nuubos/nuubos-sync/tests/sync.py
import json, os, socket, subprocess, sys, tarfile, tempfile, threading, time

here = os.path.dirname(os.path.abspath(__file__))
root = os.path.abspath(os.path.join(here, '..', '..', '..', '..'))
src = os.path.join(here, '..', 'src')
t = tempfile.mkdtemp()
run, users, state_users = t + '/run', t + '/users', t + '/state'
U1, U2 = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb'
for d in (run + '/user', users + '/' + U1, users + '/' + U2, state_users + '/' + U1, t + '/inc/cjson', t + '/inc/nuubos'):
    os.makedirs(d, exist_ok=True)
open(state_users + '/' + U1 + '/profile.conf', 'w').write('USER_ID=%s\nUSER_NAME=Mario\n' % U1)
open(t + '/model', 'w').write('Anbernic RG40XX-V\n')
with tarfile.open(root + '/dl/cjson/cjson-1.7.19.tar.gz') as tf:
    for m in tf.getmembers():
        if m.name.endswith(('/cJSON.c', '/cJSON.h')):
            m.name = os.path.basename(m.name); tf.extract(m, t + '/inc/')
open(t + '/inc/cjson/cJSON.h', 'w').write(open(t + '/inc/cJSON.h').read())
open(t + '/inc/nuubos/notify.h', 'w').write(open(root + '/package/nuubos/nuubos-notify/src/notify.h').read())

MY = 'SELF111-AAAAAAA-BBBBBBB-CCCCCCC-DDDDDDD-EEEEEEE-FFFFFFF-GGGGGGG'
PEER = 'PEER222-AAAAAAA-BBBBBBB-CCCCCCC-DDDDDDD-EEEEEEE-FFFFFFF-GGGGGGG'
NEAR = 'NEAR333-AAAAAAA-BBBBBBB-CCCCCCC-DDDDDDD-EEEEEEE-FFFFFFF-GGGGGGG'
OLD = 'OLD4444-AAAAAAA-BBBBBBB-CCCCCCC-DDDDDDD-EEEEEEE-FFFFFFF-GGGGGGG'

fake = t + '/syncthing'
open(fake, 'w').write(r'''#!/usr/bin/env python3
import http.server, json, os, socketserver, sys, threading, time, urllib.parse
args = dict(a.split("=", 1) for a in sys.argv[2:] if "=" in a)
home = args["--home"]; key = args["--gui-apikey"]; sock = args["--gui-address"][len("unix://"):]
open(home + "/ran", "a").write("x")
open(home + "/env", "w").write(os.environ.get("STMONITORED", ""))
if os.path.exists(home + "/crash"): sys.exit(3)
MY, PEER, NEAR, OLD = %r, %r, %r, %r
st = {"options": {}, "folders": {"nuubos-saves": {"id": "nuubos-saves"}}, "devices": {MY: {"deviceID": MY, "name": "nuubos"}},
      "pending": {PEER: {"name": "Laptop", "address": "192.168.1.20:22000", "time": time.strftime("%%Y-%%m-%%dT%%H:%%M:%%SZ", time.gmtime())},
                  OLD: {"name": "nuubos", "address": "127.0.0.1:22000", "time": "2026-01-01T10:00:00.123+02:00"}},
      "ignored": [],
      "offers": {"retro-saves": {"offeredBy": {PEER: {"label": "RetroArch Saves", "time": "2026-10-21T08:00:00Z", "receiveEncrypted": False}}},
                 "zz-music": {"offeredBy": {PEER: {"label": "Music", "time": "2026-10-21T08:00:00Z"}}},
                 "enc-x": {"offeredBy": {PEER: {"label": "Secret", "time": "2026-10-21T08:00:00Z", "receiveEncrypted": True}}}}}
events, cond = [], threading.Condition()
def emit(kind, data):
    with cond:
        events.append({"id": len(events) + 1, "type": kind, "data": data}); cond.notify_all()
def save(): json.dump(st, open(home + "/state.json", "w"))
save()
class H(http.server.BaseHTTPRequestHandler):
    def address_string(self): return "unix"
    def log_message(self, *a): pass
    def out(self, code, obj=None):
        b = json.dumps(obj).encode() if obj is not None else b""
        self.send_response(code); self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
    def auth(self):
        if self.headers.get("X-API-Key") != key: self.out(403); return False
        return True
    def body(self): return json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or b"null")
    def do_GET(self):
        if not self.auth(): return
        u = urllib.parse.urlparse(self.path); q = urllib.parse.parse_qs(u.query); p = u.path
        if p == "/rest/system/status": self.out(200, {"myID": MY})
        elif p == "/rest/config/devices": self.out(200, list(st["devices"].values()))
        elif p.startswith("/rest/config/devices/"):
            dv = st["devices"].get(urllib.parse.unquote(p.rpartition("/")[2]))
            self.out(200, dv) if dv else self.out(404)
        elif p == "/rest/cluster/pending/folders": self.out(200, st["offers"])
        elif p == "/rest/config/folders": self.out(200, list(st["folders"].values()))
        elif p == "/rest/config": self.out(200, {"remoteIgnoredDevices": st["ignored"], "devices": list(st["devices"].values())})
        elif p == "/rest/system/connections":
            self.out(200, {"connections": {d: {"connected": d == PEER, "address": "192.168.1.20:22000"} for d in st["devices"]}})
        elif p == "/rest/cluster/pending/devices": self.out(200, st["pending"])
        elif p == "/rest/system/discovery":
            self.out(200, {NEAR: {"addresses": ["tcp://[fe80::1]:22000", "tcp://192.168.1.30:22000"]}, MY: {"addresses": []}})
        elif p == "/rest/db/status": self.out(200, {"state": "idle"})
        elif p == "/rest/events":
            since = int(q.get("since", ["0"])[0])
            with cond:
                cond.wait_for(lambda: len(events) > since, timeout=1.0)
                self.out(200, events[since:])
        else: self.out(404)
    def do_PUT(self):
        if not self.auth(): return
        p = urllib.parse.urlparse(self.path).path; kind, _, ident = p.rpartition("/"); b = self.body()
        ident = urllib.parse.unquote(ident)
        if kind == "/rest/config/folders":
            st["folders"][ident] = b
            if st["offers"].pop(ident, None): emit("PendingFoldersChanged", {})
            if ident == "nuubos-covers":
                emit("StateChanged", {"folder": "nuubos-covers", "from": "syncing", "to": "idle"})
        elif kind == "/rest/config/devices":
            st["devices"][ident] = b; st["pending"].pop(ident, None); emit("PendingDevicesChanged", {})
        elif p == "/rest/config": st["ignored"] = b.get("remoteIgnoredDevices", [])
        else: return self.out(404)
        save(); emit("ConfigSaved", {}); self.out(200)
    def do_PATCH(self):
        if not self.auth(): return
        p = urllib.parse.urlparse(self.path).path; b = self.body()
        if p == "/rest/config/options": st["options"].update(b)
        elif p.startswith("/rest/config/devices/") and p.rpartition("/")[2] in st["devices"]:
            st["devices"][p.rpartition("/")[2]].update(b)
        else: return self.out(404)
        save(); emit("ConfigSaved", {}); self.out(200)
    def do_DELETE(self):
        if not self.auth(): return
        u = urllib.parse.urlparse(self.path); q = urllib.parse.parse_qs(u.query); p = u.path
        if p == "/rest/cluster/pending/devices":
            st["pending"].pop(q.get("device", [""])[0], None); save(); emit("PendingDevicesChanged", {}); return self.out(200)
        if p == "/rest/cluster/pending/folders":
            st["offers"].pop(q.get("folder", [""])[0], None); save(); emit("PendingFoldersChanged", {}); return self.out(200)
        kind, _, ident = p.rpartition("/"); ident = urllib.parse.unquote(ident)
        tbl = st["folders" if kind.endswith("folders") else "devices"]
        code = 200 if tbl.pop(ident, None) is not None else 404
        save(); emit("ConfigSaved", {}); self.out(code)
class S(socketserver.ThreadingMixIn, socketserver.UnixStreamServer): daemon_threads = True
try: os.unlink(sock)
except OSError: pass
srv = S(sock, H)
def later():
    time.sleep(0.5)
    emit("StateChanged", {"folder": "nuubos-mario-saves", "from": "idle", "to": "syncing"})
    emit("FolderCompletion", {"folder": "nuubos-mario-saves", "device": PEER, "completion": 42.5})
threading.Thread(target=later, daemon=True).start()
srv.serve_forever()
''' % (MY, PEER, NEAR, OLD))
os.chmod(fake, 0o755)

# Fake notifyd: collects the lines.
notes = []
nsock = run + '/notifyd.sock'
ns = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); ns.bind(nsock); ns.listen(8)
def notifyd():
    while True:
        try: c, _ = ns.accept()
        except OSError: return
        notes.append(c.recv(4096).decode()); c.close()
threading.Thread(target=notifyd, daemon=True).start()

# Fake libraryd: counts SCAN requests.
scans = []
ls = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); ls.bind(run + '/libraryd.sock'); ls.listen(8)
def libraryd():
    while True:
        try: c, _ = ls.accept()
        except OSError: return
        scans.append(c.recv(64).decode()); c.close()
threading.Thread(target=libraryd, daemon=True).start()

cc = ['gcc', '-std=c11', '-D_GNU_SOURCE', '-Wall', '-Wextra', '-Werror', '-Wno-format-truncation', '-I' + t + '/inc',
      '-DRUN_ROOT="%s"' % run, '-DUSERDATA_USERS="%s"' % users, '-DSTATE_USERS="%s"' % state_users,
      '-DMODEL_FILE="%s"' % (t + '/model'), '-DNUUBOS_NOTIFY_SOCKET="%s"' % nsock,
      '-DUSERDATA_ROOT="%s"' % (t + '/ud'), '-DLIBRARY_SOCKET="%s"' % (run + '/libraryd.sock')]
subprocess.check_call(cc + ['-DSYNCTHING_BIN="%s"' % fake, src + '/syncd.c', t + '/inc/cJSON.c', '-o', t + '/syncd',
                            '-lcurl', '-lpthread'])
subprocess.check_call(cc + [src + '/syncctl.c', '-o', t + '/syncctl'])
fails = 0
def check(n, c):
    global fails
    if not c: print('[FAIL]', n); fails += 1
def ctl(*a): return subprocess.run([t + '/syncctl'] + list(a), capture_output=True, text=True).stdout
def wait(c, s=8):
    e = time.time() + s
    while time.time() < e:
        if c(): return True
        time.sleep(0.05)
    return False
def set_user(u):
    open(run + '/user/active.tmp', 'w').write(u + '\n'); os.rename(run + '/user/active.tmp', run + '/user/active')
def fstate(): return json.load(open(users + '/' + U1 + '/appdata/syncthing/state.json'))
def conf(): return open(users + '/' + U1 + '/appdata/syncthing/nuubos.conf').read()

# Subscriber: every snapshot it receives.
snaps = []
def subscribe():
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(run + '/syncd.sock'); s.sendall(b'SUBSCRIBE\n')
    buf = ''
    while True:
        b = s.recv(65536)
        if not b: return
        buf += b.decode()
        while 'end=1\n' in buf:
            one, buf = buf.split('end=1\n', 1); snaps.append(one)
def last(): return snaps[-1] if snaps else ''

set_user(U1)
d = subprocess.Popen([t + '/syncd'], stderr=open(t + '/syncd.log', 'w'))
try:
    check('socket up', wait(lambda: os.path.exists(run + '/syncd.sock')))
    threading.Thread(target=subscribe, daemon=True).start()
    check('disabled by default', wait(lambda: 'enabled=0' in last() and 'running=0' in last()))
    check('folder before enabling (saves by default)', ctl('folder', 'states', 'on').startswith('OK')
          and 'FOLDERS=saves,states' in conf())
    check('enable', ctl('enable').startswith('OK'))
    # Syncthing waits for nuubUI (boot audit 2026-10-22).
    time.sleep(0.5)
    check('not started before ui-ready', 'running=0' in last())
    open(run + '/ui-ready', 'w').close()
    check('runs, becomes ready', wait(lambda: 'ready=1' in last()))
    check('per-user api key', 'APIKEY=' in conf())
    check('no monitor process', open(users + '/' + U1 + '/appdata/syncthing/env').read() == 'yes')
    s = fstate()
    check('device name = profile · model', s['devices'][MY]['name'] == 'Mario · RG40XX-V')
    o = s['options']
    check('local only, no reports', o.get('globalAnnounceEnabled') is False and o.get('relaysEnabled') is False
          and o.get('natEnabled') is False and o.get('crashReportingEnabled') is False and o.get('urAccepted') == -1)
    f = s['folders'].get('nuubos-mario-saves', {})
    check('saves folder per user, exFAT, trash can', f.get('path') == users + '/' + U1 + '/saves'
          and f.get('ignorePerms') is True and f.get('versioning', {}).get('type') == 'trashcan')
    check('per-user folder id and label carry the profile name', f.get('label') == 'nuubOS Saves · Mario')
    check('old shared id removed', 'nuubos-saves' not in s['folders'])
    check('states folder too', 'nuubos-mario-states' in s['folders'] and 'nuubos-mario-screenshots' not in s['folders'])
    snap = last()
    check('snapshot identity', 'device_id=' + MY in snap and 'device_name=Mario · RG40XX-V' in snap)
    check('snapshot pending + nearby (IPv4, not self)', 'pending=%s\tLaptop\t192.168.1.20' % PEER in snap
          and 'nearby=%s\t192.168.1.30' % NEAR in snap and 'nearby=' + MY not in snap)
    check('stale request dropped, no notification', 'pending=' + OLD not in snap
          and not any('sync.OLD4444' in n for n in notes) and 'OLD4444' not in json.dumps(fstate().get('pending', {})))
    check('request notification', wait(lambda: any('event=sync.request' in n and 'name=Laptop' in n for n in notes)))
    check('add (accept) device', ctl('add', PEER, 'Laptop').startswith('OK'))
    s = fstate()
    check('folders shared with it', all(any(x['deviceID'] == PEER for x in s['folders'][k]['devices'])
                                        for k in ('nuubos-mario-saves', 'nuubos-mario-states')))
    check('device listed connected, request gone', wait(lambda: 'device=%s\tLaptop\t1' % PEER in last()
                                                         and 'pending=' not in last()))
    check('request notification dismissed', wait(lambda: any(n.startswith('DISMISS id=sync.PEER222') for n in notes)))
    check('event-driven folder state + completion', wait(lambda: 'folder=saves\t1\tsyncing' in last()
                                                          and '\t1\t42\t192.168.1.20' in last()))
    r = ctl('folder', 'library', 'on')
    check('library on: covers + metadata (console store)', r.startswith('OK')
          and fstate()['folders'].get('nuubos-covers', {}).get('path') == t + '/ud/library/covers'
          and fstate()['folders'].get('nuubos-metadata', {}).get('path') == t + '/ud/library/metadata'
          and 'FOLDERS=saves,states,library' in conf())
    check('covers pulled -> library rescan', wait(lambda: any(x.startswith('SCAN') for x in scans)))
    check('history on: the user\'s library/consoles', ctl('folder', 'history', 'on').startswith('OK')
          and fstate()['folders'].get('nuubos-mario-history', {}).get('path') == users + '/' + U1 + '/library/consoles'
          and wait(lambda: 'folder=history\t1' in last() and 'folder=library\t1\t' in last()))
    check('library off removes both folders', ctl('folder', 'library', 'off').startswith('OK')
          and 'nuubos-covers' not in fstate()['folders'] and 'nuubos-metadata' not in fstate()['folders'])
    check('internet on: discovery, no relays', ctl('internet', 'on').startswith('OK')
          and fstate()['options']['globalAnnounceEnabled'] is True and fstate()['options']['relaysEnabled'] is False
          and wait(lambda: 'internet=1' in last() and 'relays=0' in last()))
    check('relays on', ctl('relays', 'on').startswith('OK') and fstate()['options']['relaysEnabled'] is True
          and 'RELAYS=1' in conf() and wait(lambda: 'relays=1' in last()))
    check('internet off also stops relays', ctl('internet', 'off').startswith('OK')
          and fstate()['options']['relaysEnabled'] is False and fstate()['options']['globalAnnounceEnabled'] is False)
    check('folder off removes it', ctl('folder', 'states', 'off').startswith('OK') and 'nuubos-mario-states' not in fstate()['folders'])
    # Existing shares of the user's other devices (user report 2026-10-21).
    check('offered folder in the snapshot, encrypted one not',
          wait(lambda: 'offer=retro-saves\tRetroArch Saves\t%s\tLaptop' % PEER in last() and 'offer=enc-x' not in last()))
    check('folderid default', 'folderid=saves\tnuubos-mario-saves\t0' in last())
    check('use offered folder as saves', ctl('use', 'retro-saves', 'saves').startswith('OK') and 'ID_saves=retro-saves' in conf())
    s = fstate()
    check('existing id on the saves path, shared, own folder removed',
          s['folders'].get('retro-saves', {}).get('path') == users + '/' + U1 + '/saves'
          and any(x['deviceID'] == PEER for x in s['folders']['retro-saves']['devices'])
          and 'nuubos-mario-saves' not in s['folders'])
    check('snapshot: custom id, offer gone', wait(lambda: 'folderid=saves\tretro-saves\t1' in last()
                                                  and 'offer=retro-saves' not in last()))
    check('folder-id with spaces/slash (URL escaped)', ctl('folder', 'states', 'on').startswith('OK')
          and ctl('folder-id', 'states', 'my states/x').startswith('OK')
          and fstate()['folders'].get('my states/x', {}).get('path') == users + '/' + U1 + '/states'
          and 'nuubos-mario-states' not in fstate()['folders'])
    check('same id for two kinds refused', ctl('folder-id', 'screenshots', 'retro-saves').startswith('ERR value'))
    check('history keeps nuubOS ids', ctl('folder-id', 'history', 'x').startswith('ERR value'))
    check('back to own saves folder, the existing one removed', ctl('folder-id', 'saves', '-').startswith('OK')
          and 'ID_saves' not in conf() and 'nuubos-mario-saves' in fstate()['folders']
          and 'retro-saves' not in fstate()['folders'])
    check('decline offer -> device ignoredFolders', ctl('decline', 'zz-music').startswith('OK')
          and any(x['id'] == 'zz-music' for x in fstate()['devices'][PEER].get('ignoredFolders', []))
          and wait(lambda: 'offer=zz-music' not in last()))
    check('states back to own', ctl('folder-id', 'states', '-').startswith('OK') and ctl('folder', 'states', 'off').startswith('OK'))
    check('ignore', ctl('ignore', NEAR).startswith('OK') and any(x['deviceID'] == NEAR for x in fstate()['ignored']))
    check('remove', ctl('remove', PEER).startswith('OK') and PEER not in fstate()['devices'])
    check('bad device id', ctl('add', '../etc').startswith('ERR device'))
    check('own id refused', ctl('add', MY).startswith('ERR device'))
    check('status text', 'end=1' in ctl('status'))
    set_user(U2)
    check('user switch stops U1 (U2 not enabled)', wait(lambda: 'running=0' in last() and 'enabled=0' in last()))
    check('U2 has no device', ctl('add', PEER).startswith('ERR not-running'))
    set_user(U1)
    check('back to U1 restarts it', wait(lambda: 'ready=1' in last()))
    # Crash: no restart storm.
    open(users + '/' + U1 + '/appdata/syncthing/crash', 'w').write('1')
    subprocess.call(['pkill', '-f', fake])
    check('crash -> failed, not restarted', wait(lambda: 'failed=1' in last() and 'running=0' in last()))
    runs = open(users + '/' + U1 + '/appdata/syncthing/ran').read().count('x')
    time.sleep(1)
    check('no restart loop', open(users + '/' + U1 + '/appdata/syncthing/ran').read().count('x') == runs)
    os.unlink(users + '/' + U1 + '/appdata/syncthing/crash')
    check('enable again clears failed', ctl('enable').startswith('OK') and wait(lambda: 'ready=1' in last()))
    check('disable', ctl('disable').startswith('OK') and wait(lambda: 'running=0' in last()))
    # Spaces and accents: lowercased ASCII, "-" for punctuation, hex for UTF-8.
    open(state_users + '/' + U1 + '/profile.conf', 'w').write('USER_ID=%s\nUSER_NAME=Ana María\n' % U1)
    check('renamed profile gets its own folder id', ctl('enable').startswith('OK') and wait(lambda: 'ready=1' in last())
          and wait(lambda: 'nuubos-ana-marc3ada-saves' in fstate()['folders']))
    check('old name\'s folders removed (no overlapping paths)', 'nuubos-mario-saves' not in fstate()['folders']
          and 'nuubos-mario-history' not in fstate()['folders'])
    check('disable again', ctl('disable').startswith('OK') and wait(lambda: 'running=0' in last()))
finally:
    d.terminate(); d.wait(); subprocess.call(['pkill', '-f', fake]); ns.close(); ls.close()
if fails:
    print(open(t + '/syncd.log').read())
print('[PASS] sync' if not fails else '[FAIL] %d checks' % fails)
sys.exit(1 if fails else 0)
