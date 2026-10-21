#!/usr/bin/env python3
# Host test: nuubos-tsd downloads Tailscale through its helper (progress,
# turned on afterwards), runs tailscaled only while on, applies the nuubOS
# policy through the local API (with the Sec-Tailscale header), follows the
# event bus (sign-in URL, Running with name/addresses), signs in/out, checks
# for updates, removes the build and the node state, and does not restart a
# tailscaled that died. Fake tailscaled (local API on the unix socket) and
# fake download helper. nuubos-tsctl is its command line.
# Run: python3 package/nuubos/nuubos-tailscale/tests/tailscale.py
import json, os, subprocess, sys, tarfile, tempfile, time

here = os.path.dirname(os.path.abspath(__file__))
root = os.path.abspath(os.path.join(here, '..', '..', '..', '..'))
src = os.path.join(here, '..', 'src')
t = tempfile.mkdtemp()
run, app, state, conf = t + '/run', t + '/ud/tailscale', t + '/state/tailscale', t + '/state/config/tailscale.conf'
for d in (run, t + '/ud', t + '/inc/cjson'):
    os.makedirs(d, exist_ok=True)
open(t + '/model', 'w').write('Anbernic RG CubeXX\n')
with tarfile.open(root + '/dl/cjson/cjson-1.7.19.tar.gz') as tf:
    for m in tf.getmembers():
        if m.name.endswith(('/cJSON.c', '/cJSON.h')):
            m.name = os.path.basename(m.name); tf.extract(m, t + '/inc/')
open(t + '/inc/cjson/cJSON.h', 'w').write(open(t + '/inc/cJSON.h').read())

# Fake tailscaled: the local API calls nuubos-tsd makes.
fake = t + '/fake-tailscaled'
open(fake, 'w').write(r'''#!/usr/bin/env python3
import http.server, json, os, socketserver, sys, threading, time, urllib.parse
args = dict(a.split("=", 1) for a in sys.argv[1:] if "=" in a)
sock, statedir = args["--socket"], args["--statedir"]
open(statedir + "/args", "w").write(" ".join(sys.argv[1:]))
open(statedir + "/ran", "a").write("x")
if os.path.exists(statedir + "/crash"): sys.exit(3)
st = {"state": "NeedsLogin", "url": "", "prefs": {}, "logouts": 0}
cond = threading.Condition(); gen = [0]
def change(**kw):
    with cond:
        st.update(kw); gen[0] += 1; cond.notify_all()
    json.dump(st, open(statedir + "/fake.json", "w"))
change()
class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def address_string(self): return "unix"
    def log_message(self, *a): pass
    def out(self, code, obj=None):
        b = json.dumps(obj).encode() if obj is not None else b""
        self.send_response(code); self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
    def ok(self):
        if self.headers.get("Sec-Tailscale") != "localapi" or self.headers.get("Host") != "local-tailscaled.sock":
            self.out(403, {"error": "missing header"}); return False
        return True
    def body(self): return json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or b"null")
    def do_GET(self):
        if not self.ok(): return
        p = urllib.parse.urlparse(self.path).path
        if p == "/localapi/v0/status":
            run = st["state"] == "Running"
            self.out(200, {"BackendState": st["state"], "AuthURL": st["url"],
                           "Self": {"DNSName": "nuubos-rg-cubexx.tail1234.ts.net." if run else "",
                                    "TailscaleIPs": ["100.101.102.103", "fd7a:115c:a1e0::1"] if run else []},
                           "CurrentTailnet": {"Name": "mario@example.com"} if run else None})
        elif p == "/localapi/v0/watch-ipn-bus":
            self.send_response(200); self.send_header("Content-Type", "application/json"); self.end_headers()
            seen = -1
            try:
                while True:
                    with cond:
                        cond.wait_for(lambda: gen[0] != seen, timeout=30)
                        seen = gen[0]
                    self.wfile.write((json.dumps({"State": 2}) + "\n").encode()); self.wfile.flush()
            except OSError: return
        else: self.out(404)
    def do_PATCH(self):
        if not self.ok(): return
        if urllib.parse.urlparse(self.path).path == "/localapi/v0/prefs":
            st["prefs"].update(self.body()); change(); self.out(200, st["prefs"])
        else: self.out(404)
    def do_POST(self):
        if not self.ok(): return
        p = urllib.parse.urlparse(self.path).path
        self.rfile.read(int(self.headers.get("Content-Length", 0)))
        if p == "/localapi/v0/login-interactive":
            change(url="https://login.tailscale.com/a/abc123")
            def later(): time.sleep(0.7); change(state="Running", url="")
            threading.Thread(target=later, daemon=True).start(); self.out(204)
        elif p == "/localapi/v0/logout":
            st["logouts"] += 1; change(state="NeedsLogin"); self.out(204)
        else: self.out(404)
class S(socketserver.ThreadingMixIn, socketserver.UnixStreamServer): daemon_threads = True
try: os.unlink(sock)
except OSError: pass
S(sock, H).serve_forever()
''')
os.chmod(fake, 0o755)

# Fake download helper.
get = t + '/fake-get'
open(get, 'w').write('''#!/bin/sh
case "$1" in
check) echo "@latest 1.200.0" ;;
install)
    [ -e "%(t)s/fail-install" ] && { echo "@error checksum"; exit 1; }
    for p in 10 50 99; do echo "@progress $p"; sleep 0.1; done
    mkdir -p "$2/app" && cp "%(fake)s" "$2/app/tailscaled" && printf 1.104.2 > "$2/app/VERSION"
    echo "@done 1.104.2" ;;
remove) rm -rf "$2/app"; echo "@done" ;;
esac
''' % {'t': t, 'fake': fake})
os.chmod(get, 0o755)

cc = ['gcc', '-std=c11', '-D_GNU_SOURCE', '-Wall', '-Wextra', '-Werror', '-Wno-format-truncation', '-I' + t + '/inc',
      '-DRUN_ROOT="%s"' % run, '-DAPP_ROOT="%s"' % app, '-DSTATE_DIR="%s"' % state, '-DCONF_PATH="%s"' % conf,
      '-DGET_BIN="%s"' % get, '-DMODEL_FILE="%s"' % (t + '/model')]
subprocess.check_call(cc + [src + '/tsd.c', t + '/inc/cJSON.c', '-o', t + '/tsd', '-lcurl', '-lpthread'])
subprocess.check_call(cc + [src + '/tsctl.c', '-o', t + '/tsctl'])
fails = 0
def check(n, c):
    global fails
    if not c: print('[FAIL]', n); fails += 1
def ctl(*a): return subprocess.run([t + '/tsctl'] + list(a), capture_output=True, text=True).stdout
def snap(): return ctl('status')
def wait(c, s=8):
    e = time.time() + s
    while time.time() < e:
        if c(): return True
        time.sleep(0.05)
    return False
def fake_state(): return json.load(open(state + '/fake.json'))

d = subprocess.Popen([t + '/tsd'], stderr=open(t + '/tsd.log', 'w'))
try:
    check('socket up', wait(lambda: os.path.exists(run + '/tsd.sock')))
    s = snap()
    check('not installed, off', 'installed=0' in s and 'enabled=0' in s and 'running=0' in s)
    check('enable needs the build', ctl('enable').startswith('ERR not-installed'))
    check('check -> latest', ctl('check').startswith('OK') and wait(lambda: 'latest=1.200.0' in snap() and 'job=\n' in snap()))
    check('install starts', ctl('install').startswith('OK') and 'job=install' in snap())
    check('second job refused while busy', ctl('check').startswith('ERR busy'))
    check('installed, version, turned on', wait(lambda: 'installed=1' in snap() and 'version=1.104.2' in snap()
                                                 and 'enabled=1' in snap()) and 'ENABLED=1' in open(conf).read())
    check('tailscaled ready', wait(lambda: 'ready=1' in snap()))
    a = open(state + '/args').read()
    check('tailscaled arguments', '--statedir=' + state in a and '--no-logs-no-support' in a and '--tun=tailscale0' in a)
    check('policy through the local API', wait(lambda: fake_state()['prefs'].get('CorpDNSSet') is True))
    p = fake_state()['prefs']
    check('policy values', p.get('CorpDNS') is False and p.get('NetfilterMode') == 0 and p.get('WantRunning') is True
          and p.get('Hostname') == 'nuubOS RG CubeXX' and p.get('RouteAll') is False)
    check('needs login', wait(lambda: 'state=NeedsLogin' in snap()))
    check('login -> sign-in URL', ctl('login').startswith('OK')
          and wait(lambda: 'auth_url=https://login.tailscale.com/a/abc123' in snap(), 3))
    check('running: name, addresses, tailnet (event bus)', wait(lambda: 'state=Running' in snap()
          and 'name=nuubos-rg-cubexx.tail1234.ts.net\n' in snap() and 'ip=100.101.102.103' in snap()
          and 'ip6=fd7a:115c:a1e0::1' in snap() and 'tailnet=mario@example.com' in snap() and 'auth_url=\n' in snap()))
    check('logout', ctl('logout').startswith('OK') and wait(lambda: 'state=NeedsLogin' in snap() and 'name=\n' in snap()))
    check('disable stops it', ctl('disable').startswith('OK') and wait(lambda: 'running=0' in snap()) and 'ENABLED=0' in open(conf).read())
    check('login when off refused', ctl('login').startswith('ERR not-running'))
    check('enable starts it again', ctl('enable').startswith('OK') and wait(lambda: 'ready=1' in snap()))
    # Crash: no restart storm.
    open(state + '/crash', 'w').write('1')
    runs = open(state + '/ran').read().count('x')
    subprocess.call(['pkill', '-f', app + '/app/tailscaled'])
    check('crash -> failed, not restarted', wait(lambda: 'failed=1' in snap() and 'running=0' in snap()))
    time.sleep(0.5)
    check('no restart loop', open(state + '/ran').read().count('x') == runs)
    os.unlink(state + '/crash')
    check('enable clears failed', ctl('enable').startswith('OK') and wait(lambda: 'ready=1' in snap() and 'failed=0' in snap()))
    open(t + '/fail-install', 'w').write('1')
    check('failed update keeps the build and runs it again', ctl('install').startswith('OK')
          and wait(lambda: 'error=checksum' in snap() and 'job=\n' in snap()) and wait(lambda: 'ready=1' in snap()))
    os.unlink(t + '/fail-install')
    check('remove: build, state and on/off gone', ctl('remove').startswith('OK')
          and wait(lambda: 'installed=0' in snap() and 'job=\n' in snap()) and not os.path.exists(state)
          and 'ENABLED=0' in open(conf).read() and 'running=0' in snap())
    check('remove when absent', ctl('remove').startswith('ERR not-installed'))
    check('unknown command', ctl('frobnicate') == '')
finally:
    d.terminate(); d.wait(); subprocess.call(['pkill', '-f', 'fake-tailscaled'])
    subprocess.call(['pkill', '-f', app + '/app/tailscaled'])
if fails:
    print(open(t + '/tsd.log').read())
print('[PASS] tailscale' if not fails else '[FAIL] %d checks' % fails)
sys.exit(1 if fails else 0)
