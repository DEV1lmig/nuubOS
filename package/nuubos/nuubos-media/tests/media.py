#!/usr/bin/env python3
# Host test: nuubos-mediad session control against a fake mpv speaking the
# JSON IPC on fd 3, and nuubos-jellyfinctl against a fake Jellyfin server.
# Run: python3 package/nuubos/nuubos-media/tests/media.py
import http.server, json, os, socket, subprocess, sys, tarfile, tempfile, threading, time

here = os.path.dirname(os.path.abspath(__file__))
root = os.path.abspath(os.path.join(here, '..', '..', '..', '..'))
src = os.path.join(here, '..', 'src')
t = tempfile.mkdtemp()
run, users, state = t + '/run', t + '/users', t + '/state'
user = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'
for d in (run + '/user', users + '/' + user, state + '/' + user, t + '/inc/nuubos', t + '/cj/cjson', t + '/music'):
    os.makedirs(d, exist_ok=True)
open(run + '/user/active', 'w').write(user + '\n')
open(t + '/inc/nuubos/notify.h', 'w').write(open(os.path.join(root, 'package/nuubos/nuubos-notify/src/notify.h')).read())
with tarfile.open(root + '/dl/cjson/cjson-1.7.19.tar.gz') as tf:
    for m in tf.getmembers():
        if m.name.endswith(('/cJSON.c', '/cJSON.h')):
            m.name = os.path.basename(m.name); tf.extract(m, t + '/cj/')
open(t + '/cj/cjson/cJSON.h', 'w').write(open(t + '/cj/cJSON.h').read())
for n in ('b.mp3', 'a.flac', 'c.txt', 'd.ogg'):
    open(t + '/music/' + n, 'w').write('x')
os.makedirs(t + '/pl')
for n in ('1-short.mp3', '2-short.mp3', 'x-broken.mp3', 'y-broken.mp3'):
    open(t + '/pl/' + n, 'w').write('x')
# 24-bit stereo 96 kHz STREAMINFO: rate 20 bits, channels-1 3 bits, bps-1 5 bits.
si = bytearray(34); v = (96000 << 12) | (1 << 9) | (23 << 4); si[10:14] = v.to_bytes(4, 'big')
open(t + '/pl/hires.flac', 'wb').write(b'fLaC' + bytes([0x80, 0, 0, 34]) + bytes(si))
open(t + '/movie.mkv', 'w').write('x')

fake_mpv = t + '/mpv'
open(fake_mpv, 'w').write('''#!/usr/bin/env python3
# Fake mpv: video = the file on the command line; audio (--idle=yes) =
# files given by loadfile. Files named *short* end at once (eof).
import json, os, socket, sys
args = sys.argv[1:]
open(os.environ.get("HOME", "/tmp") + "/args", "w").write("\\n".join(args))
s = socket.socket(fileno=3)
f = s.makefile("rw")
paused = False
idle = "--idle=yes" in args
cur = None if idle else args[-1]
def send(o):
    f.write(json.dumps(o) + "\\n"); f.flush()
def ev(name, data):
    send({"event": "property-change", "name": name, "data": data})
def load(path, opts=""):
    global cur
    if cur: send({"event": "end-file", "reason": "stop"})
    cur = path
    title = opts.split("%", 2)[2] if opts.startswith("force-media-title=") else os.path.basename(path)
    open(os.environ.get("HOME", "/tmp") + "/loaded", "a").write(path + "\\n")
    ev("media-title", title); ev("duration", 200.0)
    ev("metadata", {"ARTIST": "Band", "Album": "Record"})
    ev("audio-codec-name", "flac" if path.endswith(".flac") else "mp3"); ev("audio-params/samplerate", 96000)
    send({"event": "playback-restart"})
    if "short" in path or "broken" in path:
        cur = None
        send({"event": "end-file", "reason": "error" if "broken" in path else "eof"})
for line in f:
    m = json.loads(line); c = m.get("command", [])
    if isinstance(c, dict):
        open(os.environ.get("HOME", "/tmp") + "/osd", "a").write(json.dumps(c) + "\\n")
        continue
    if c[:1] == ["observe_property"]:
        n = c[2]
        if cur is None: continue
        if n == "path": ev("path", cur)
        elif n == "media-title": ev("media-title", os.path.basename(cur))
        elif n == "pause": ev("pause", False)
        elif n == "playlist-pos": ev("playlist-pos", 0)
        elif n == "playlist-count": ev("playlist-count", 1)
        elif n == "vid": ev("vid", 1 if cur.endswith(".mkv") else False)
        elif n == "osd-width": ev("osd-width", 720)
        elif n == "osd-height": ev("osd-height", 720)
        if n == "vid": send({"event": "playback-restart"})
    elif c[:1] == ["loadfile"]:
        load(c[1], c[4] if len(c) > 4 else "")
    elif c == ["cycle", "pause"]:
        paused = not paused; ev("pause", paused)
    elif c[:2] == ["set_property", "pause"]:
        paused = c[2]; ev("pause", paused)
    elif c[:1] == ["seek"]:
        open(os.environ.get("HOME", "/tmp") + "/seeks", "a").write("%s %s\\n" % (c[1], c[2]))
        send({"event": "playback-restart"})
    elif c[:1] == ["get_property"]:
        send({"data": 42.5, "request_id": m.get("request_id"), "error": "success"})
    elif c in (["quit-watch-later"], ["quit"]):
        sys.exit(0)
''')
os.chmod(fake_mpv, 0o755)

common = ['-std=c11', '-D_GNU_SOURCE', '-Wall', '-Wextra', '-Werror', '-Wno-format-truncation', '-I' + t + '/inc', '-I' + t + '/cj']
subprocess.check_call(['gcc'] + common + ['-DRUN_ROOT="%s"' % run, '-DMPV_BIN="%s"' % fake_mpv,
    '-DUSERDATA_USERS="%s"' % users, '-DNUUBOS_NOTIFY_SOCKET="%s/notifyd.sock"' % run,
    src + '/mediad.c', t + '/cj/cJSON.c', '-o', t + '/mediad'])
subprocess.check_call(['gcc'] + common + ['-DRUN_ROOT="%s"' % run, src + '/playerctl.c', '-o', t + '/playerctl'])
subprocess.check_call(['gcc'] + common + ['-DSTATE_USERS="%s"' % state, '-DIMAGE_CACHE="%s/jfcache"' % t, src + '/jellyfinctl.c', t + '/cj/cJSON.c', '-o', t + '/jf', '-lcurl'])
ud, media = t + '/ud', t + '/media'
subprocess.check_call(['gcc'] + common + ['-DUSERDATA_ROOT="%s"' % ud, '-DMEDIA_ROOT="%s"' % media, '-DSHARES_ROOT="%s/shares"' % t, src + '/medialib.c', '-o', t + '/ml'])

fails = 0
def check(name, cond):
    global fails
    if not cond:
        print('[FAIL]', name); fails += 1
def pc(*a):
    return subprocess.run([t + '/playerctl'] + list(a), capture_output=True, text=True).stdout
def status():
    return dict(l.split('=', 1) for l in pc('status').splitlines() if '=' in l)
def wait(cond, secs=5):
    end = time.time() + secs
    while time.time() < end:
        if cond(): return True
        time.sleep(0.05)
    return False

d = subprocess.Popen([t + '/mediad'], stderr=open(t + '/mediad.log', 'w'))
try:
    wait(lambda: os.path.exists(run + '/mediad.sock'))
    check('missing file refused', pc('play', t + '/nope.mkv').startswith('ERR file'))
    check('play video', pc('play', t + '/movie.mkv', '30').startswith('OK'))
    check('state playing, video', wait(lambda: status().get('state') == 'playing' and status().get('kind') == 'video'))
    args = open(users + '/' + user + '/appdata/media/args').read()
    check('cedrus decode, gpu output', '--hwdec=v4l2request' in args and '--vo=gpu' in args)
    check('per-user resume', 'watch-later' in args and '--save-position-on-quit=yes' in args and '--start=30' in args)
    check('video resume lists its file', '--write-filename-in-watch-later-config=yes' in args)
    osd = lambda: open(users + '/' + user + '/appdata/media/osd').read() if os.path.exists(users + '/' + user + '/appdata/media/osd') else ''
    check('osd shown at start', wait(lambda: 'ass-events' in osd()))
    check('labels', pc('labels', 'Pausa', 'Riproduci', 'Sottotitoli', 'Audio', 'Stop', 'No', '2', '1', '0', '3').startswith('OK'))
    open(users + '/' + user + '/appdata/media/osd', 'w').close()
    check('key while shown acts', pc('key', 'menu_confirm').startswith('OK') and wait(lambda: status().get('state') == 'paused'))
    check('osd localized, paused sign', wait(lambda: 'Riproduci' in osd() and 'Sottotitoli' in osd()))
    check('osd canvas follows a square screen', '"res_x": 720' in osd())
    mh = users + '/' + user + '/appdata/media/'
    open(mh + 'seeks', 'w').close()
    for k in range(4):
        pc('key', 'menu_right')
    sk = open(mh + 'seeks').read() if wait(lambda: open(mh + 'seeks').read().count('relative') >= 2) else ''
    check('1 s exact, then held steps', sk.startswith('1 relative+exact') and 'relative+keyframes' in sk)
    pc('pause')
    check('stop', pc('stop').startswith('OK'))
    check('idle after stop + position kept', wait(lambda: status().get('state') == 'idle') and status().get('last_position') == '42' and status().get('last_item', '').endswith('movie.mkv'))
    home = users + '/' + user + '/appdata/media/'
    check('playdir', pc('playdir', t + '/music', 'b.mp3').startswith('OK'))
    check('audio queue from file', wait(lambda: status().get('kind') == 'audio' and status().get('count') == '3' and status().get('index') == '1' and status().get('item', '').endswith('b.mp3')))
    check('music has no window', wait(lambda: '--idle=yes' in open(home + 'args').read()))
    args = open(home + 'args').read()
    check('music args', '--force-window=no' in args and '--vid=no' in args and '--fullscreen' not in args
          and '--audio-client-name=nuubOS Music' in args and '--volume=80' in args)
    check('music volume', pc('volume', '35').startswith('OK') and status().get('volume') == '35'
          and 'MUSIC_VOLUME=35' in open(home + 'settings.conf').read())
    check('volume range', pc('volume', '101').startswith('ERR'))
    check('tags and duration', wait(lambda: status().get('artist') == 'Band' and status().get('album') == 'Record' and status().get('duration') == '200.0'))
    q = [l.split('\t') for l in pc('queue').splitlines() if l.startswith('track=')]
    check('queue: only audio files, sorted', [os.path.basename(x[1]) for x in q] == ['a.flac', 'b.mp3', 'd.ogg'])
    check('next', pc('next').startswith('OK') and wait(lambda: status().get('item', '').endswith('d.ogg')))
    check('next at the end stays', pc('next').startswith('OK') and status().get('item', '').endswith('d.ogg'))
    check('repeat cycles', pc('repeat').startswith('OK') and status().get('repeat') == 'all')
    check('next wraps with repeat all', pc('next').startswith('OK') and wait(lambda: status().get('item', '').endswith('a.flac')))
    pc('repeat'); pc('repeat')
    check('repeat back to off', status().get('repeat') == 'off')
    check('jump', pc('jump', '2').startswith('OK') and wait(lambda: status().get('item', '').endswith('d.ogg') and status().get('index') == '2'))
    qid = status().get('queue_id')
    check('shuffle keeps the track first', pc('shuffle').startswith('OK') and status().get('shuffle') == '1' and status().get('index') == '0' and status().get('item', '').endswith('d.ogg') and status().get('queue_id') != qid)
    sq = [os.path.basename(l.split('\t')[1]) for l in pc('queue').splitlines() if l.startswith('track=')]
    check('shuffled queue: same tracks', sorted(sq) == ['a.flac', 'b.mp3', 'd.ogg'] and sq[0] == 'd.ogg')
    check('unshuffle', pc('shuffle').startswith('OK') and status().get('shuffle') == '0' and status().get('index') == '2')
    check('seekto absolute', pc('seekto', '30').startswith('OK') and wait(lambda: os.path.exists(home + 'seeks') and '30 absolute' in open(home + 'seeks').read()))
    # Replacing music with a video: stop first, then the new session.
    check('replace while playing', pc('play', t + '/movie.mkv').startswith('OK'))
    check('video after the music', wait(lambda: status().get('kind') == 'video' and status().get('state') == 'playing'))
    pc('stop'); wait(lambda: status().get('state') == 'idle')
    # PLAYLIST: lines after the command; short tracks end at once.
    open(home + 'loaded', 'w').close()
    sock = socket.socket(socket.AF_UNIX); sock.connect(run + '/mediad.sock')
    body = 'PLAYLIST\t0\t3\n%s/pl/1-short.mp3\tFirst, Song=1\n/nope.mp3\n%s/pl/2-short.mp3\n' % (t, t)
    sock.sendall(body.encode()); sock.settimeout(3)
    check('playlist accepted', sock.recv(100).startswith(b'OK')); sock.close()
    check('eof plays the next, then the queue ends', wait(lambda: status().get('state') == 'idle'))
    loaded = open(home + 'loaded').read().split()
    check('missing entries dropped, order kept', [os.path.basename(x) for x in loaded] == ['1-short.mp3', '2-short.mp3'])
    sock = socket.socket(socket.AF_UNIX); sock.connect(run + '/mediad.sock')
    sock.sendall(('PLAYLIST\t0\t2\n%s/pl/x-broken.mp3\n%s/pl/y-broken.mp3\n' % (t, t)).encode()); sock.settimeout(3)
    check('broken playlist accepted', sock.recv(100).startswith(b'OK')); sock.close()
    check('all broken: stops', wait(lambda: status().get('state') == 'idle'))
    check('hi-res flac', pc('play', t + '/pl/hires.flac').startswith('OK') and wait(lambda: status().get('bits') == '24'
          and status().get('codec') == 'flac' and status().get('rate') == '96000'))
    check('playing mark', wait(lambda: os.path.exists(run + '/media-playing')))
    check('paused: no mark', pc('pause').startswith('OK') and wait(lambda: not os.path.exists(run + '/media-playing')))
    pc('stop'); wait(lambda: status().get('state') == 'idle')
    check('url with audio flag', pc('play', 'http://127.0.0.1:9/x', '0', 'audio').startswith('OK') and wait(lambda: status().get('kind') == 'audio'))
    check('pre-power stops and replies', pc('pre-power').startswith('OK') and status().get('state') == 'idle')
    check('commands need a session', pc('pause').startswith('ERR no-session'))
finally:
    d.terminate(); d.wait()

# Library (nuubos-medialib)
import struct
JPEG = b'\xff\xd8\xff\xe0' + b'J' * 40
PNG = b'\x89PNG\r\n\x1a\n' + b'P' * 40
def ss(n): return bytes([(n >> 21) & 127, (n >> 14) & 127, (n >> 7) & 127, n & 127])
def id3(pic):
    body = b'\x00image/jpeg\x00\x03cover\x00' + pic
    frame = b'APIC' + struct.pack('>I', len(body)) + b'\x00\x00' + body
    return b'ID3\x03\x00\x00' + ss(len(frame)) + frame + b'\xff\xfbaudio'
def flac(pic):
    blk = struct.pack('>I', 3) + struct.pack('>I', 9) + b'image/png' + struct.pack('>I', 0) + b'\x00' * 16 + struct.pack('>I', len(pic)) + pic
    return b'fLaC' + bytes([0x00]) + struct.pack('>I', 34)[1:] + b'\x00' * 34 + bytes([0x86]) + struct.pack('>I', len(blk))[1:] + blk
def atom(t, body): return struct.pack('>I', 8 + len(body)) + t + body
def m4a(pic):
    data = atom(b'data', b'\x00\x00\x00\x0d\x00\x00\x00\x00' + pic)
    meta = atom(b'meta', b'\x00\x00\x00\x00' + atom(b'hdlr', b'\x00' * 24) + atom(b'ilst', atom(b'covr', data)))
    return atom(b'ftyp', b'M4A ') + atom(b'mdat', b'\x00' * 64) + atom(b'moov', atom(b'mvhd', b'\x00' * 20) + atom(b'udta', meta))
for d in ('videos/Show', 'music/Artist/Album', 'music/Loose', 'users/%s/recordings' % user, 'users/%s/appdata/media/watch-later' % user):
    os.makedirs(ud + '/' + d)
os.makedirs(t + '/shares/NAS/Video'); os.makedirs(media + '/USB1/Movies'); os.makedirs(media + '/USB1/MUSIC'); os.makedirs(media + '/sd2/roms')
open(ud + '/videos/Film.mkv', 'w').write('x'); open(ud + '/videos/Film.jpg', 'w').write('x')
open(ud + '/videos/notes.txt', 'w').write('x')
open(ud + '/videos/Show/E01.mp4', 'w').write('x'); open(ud + '/videos/Show/poster.PNG', 'w').write('x')
open(ud + '/music/Artist/Album/01 - Intro.mp3', 'wb').write(id3(JPEG))
open(ud + '/music/Artist/Album/02. Song_Two.mp3', 'wb').write(b'\xff\xfbnothing')
open(ud + '/music/Loose/a.flac', 'wb').write(flac(PNG))
open(ud + '/music/Loose/b.m4a', 'wb').write(m4a(JPEG))
open(ud + '/music/x.ogg', 'w').write('x')
wl = ud + '/users/%s/appdata/media/watch-later/' % user
open(wl + 'AAAA', 'w').write('# %s/videos/Film.mkv\nstart=754.250000\n' % ud)
open(wl + 'BBBB', 'w').write('# /gone/file.mkv\nstart=10.000000\n')
ml = lambda *a: subprocess.run([t + '/ml'] + list(a), capture_output=True, text=True).stdout
out = ml('sources', user)
check('sources: internal, recordings, volumes', ('source=videos\t%s/videos\t\n' % ud) in out and ('source=music\t%s/music\t\n' % ud) in out
      and ('source=recordings\t%s/users/%s/recordings\t\n' % (ud, user)) in out
      and ('source=videos\t%s/shares/NAS/Video\tshare:NAS' % t) in out
      and ('source=videos\t%s/USB1/Movies\tUSB1' % media) in out and ('source=music\t%s/USB1/MUSIC\tUSB1' % media) in out and 'roms' not in out)
out = ml('videos', ud + '/videos', user)
check('videos: folder with poster', ('folder=%s/videos/Show\tShow\t%s/videos/Show/poster.PNG\t1' % (ud, ud)) in out)
check('videos: file, poster, resume', ('video=%s/videos/Film.mkv\tFilm\t%s/videos/Film.jpg\t754\t1' % (ud, ud)) in out and 'notes' not in out)
out = ml('albums', ud + '/music')
lines = [l.split('\t') for l in out.splitlines() if l.startswith('album=')]
alb = {l[0][6:]: l for l in lines}
a1 = alb.get(ud + '/music/Artist/Album')
check('album with artist and embedded ID3 cover', a1 is not None and a1[1] == 'Album' and a1[2] == 'Artist' and a1[3].endswith('.jpg') and a1[4] == '2'
      and open(a1[3], 'rb').read() == JPEG)
a2 = alb.get(ud + '/music/Loose')
check('FLAC picture', a2 is not None and a2[2] == '' and a2[3].endswith('.png') and open(a2[3], 'rb').read() == PNG)
check('loose tracks = album without title', alb.get(ud + '/music', ['', 'x'])[1] == '')
out = ml('cover', ud + '/music/Loose/b.m4a')
check('MP4 covr', out.startswith('cover=') and out.strip().endswith('.jpg') and open(out.strip()[6:], 'rb').read() == JPEG)
check('no picture remembered', ml('cover', ud + '/music/Artist/Album/02. Song_Two.mp3').strip() == 'cover=' and
      any(n.endswith('.none') for n in os.listdir(ud + '/cache/media/covers')))
out = ml('tracks', ud + '/music/Artist/Album')
check('track titles and numbers', ('track=%s/music/Artist/Album/01 - Intro.mp3\tIntro\t1' % ud) in out and ('\tSong Two\t2' in out))
out = ml('tracks', ud + '/music', 'all')
check('all tracks below', out.count('track=') == 5)
out = ml('resume', user)
check('resume: existing local videos', ('resume=%s/videos/Film.mkv\t754\t' % ud) in out and '/gone/' not in out and 'Film.jpg' in out)
check('outside user storage refused', ml('videos', '/etc').startswith('ERR'))
out = ml('playlist-new', user, 'Road Trip')
pl = out.strip()[3:]
check('playlist created', out.startswith('OK ') and pl.endswith('/appdata/media/playlists/Road Trip.m3u'))
check('playlist name taken', ml('playlist-new', user, 'Road Trip').startswith('ERR exists'))
check('bad playlist name', ml('playlist-new', user, '../x').startswith('ERR name'))
check('add album + track, no repeats', ml('playlist-add', pl, ud + '/music/Artist/Album', ud + '/music/x.ogg', ud + '/music/x.ogg', '/etc/passwd').strip() == 'OK 3')
out = ml('playlist', pl)
check('playlist tracks in order', [l.split('\t')[1] for l in out.splitlines() if l.startswith('track=')] == ['Intro', 'Song Two', 'x'])
check('m3u for other players', open(pl).read().startswith('#EXTM3U\n#EXTINF:-1,Intro\n'))
check('remove', ml('playlist-remove', pl, '1').strip() == 'OK 2')
out = ml('playlists', user)
check('playlists with cover', ('playlist=%s\tRoad Trip\t2\t' % pl) in out and '.jpg' in out)
out = ml('playlist-rename', pl, 'Trip')
check('rename', out.startswith('OK ') and out.strip().endswith('Trip.m3u') and not os.path.exists(pl))
pl = out.strip()[3:]
check('foreign file refused', ml('playlist-delete', '/etc/passwd').startswith('ERR') and ml('playlist-delete', ud + '/music/x.ogg').startswith('ERR'))
check('delete', ml('playlist-delete', pl).startswith('OK') and not os.path.exists(pl))

# Jellyfin
seen = []
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def reply(self, code, obj=None):
        self.send_response(code); self.send_header('Content-Type', 'application/json'); self.end_headers()
        if obj is not None: self.wfile.write(json.dumps(obj).encode())
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get('Content-Length', 0))).decode()
        seen.append((self.path, body, self.headers.get('X-Emby-Authorization')))
        if self.path == '/Users/AuthenticateByName':
            b = json.loads(body)
            if b['Pw'] == 'secret': self.reply(200, {'AccessToken': 'tok', 'User': {'Id': 'u1'}})
            else: self.reply(401)
        elif self.path == '/Sessions/Playing/Stopped': self.reply(204)
    def do_GET(self):
        seen.append((self.path, '', self.headers.get('X-Emby-Authorization')))
        if 'Token="tok"' not in (self.headers.get('X-Emby-Authorization') or ''): return self.reply(401)
        if self.path == '/Users/u1/Views':
            self.reply(200, {'Items': [{'Id': 'lib1', 'Name': 'Movies', 'Type': 'CollectionFolder', 'CollectionType': 'movies', 'IsFolder': True}]})
        elif self.path.startswith('/Users/u1/Items/Resume'):
            self.reply(200, {'Items': [{'Id': 'e1', 'Name': 'Pilot', 'Type': 'Episode', 'IsFolder': False, 'SeriesName': 'Show', 'IndexNumber': 1,
                                        'RunTimeTicks': 6000000000, 'UserData': {'PlaybackPositionTicks': 600000000}}]})
        elif self.path.startswith('/Items/m1/Images/Primary'):
            self.send_response(200); self.send_header('Content-Type', 'image/jpeg'); self.end_headers(); self.wfile.write(JPEG)
        elif self.path.startswith('/Items/m2/Images/Primary'):
            self.reply(404)
        elif self.path.startswith('/Users/u1/Items?ParentId=lib1'):
            self.reply(200, {'Items': [{'Id': 'm1', 'Name': 'Big\tBuck', 'Type': 'Movie', 'IsFolder': False,
                                        'RunTimeTicks': 6000000000, 'UserData': {'PlaybackPositionTicks': 1200000000}}]})
srv = http.server.HTTPServer(('127.0.0.1', 0), H); port = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()
jf = lambda *a, stdin=None: subprocess.run([t + '/jf'] + list(a), input=stdin, capture_output=True, text=True).stdout
url = 'http://127.0.0.1:%d' % port
check('wrong password', jf('login', user, url, 'bob', stdin='nope\n').startswith('ERR auth'))
check('login', jf('login', user, url, 'bob', stdin='secret\n').startswith('OK'))
conf = open(state + '/' + user + '/secrets/jellyfin.conf').read()
check('token stored, password not', 'TOKEN=tok' in conf and 'secret' not in conf)
check('creds root-only', (os.stat(state + '/' + user + '/secrets/jellyfin.conf').st_mode & 0o077) == 0)
check('views with library kind', 'item=lib1\tMovies\tCollectionFolder\t1\t0\t0\tmovies\t0' in jf('views', user))
out = jf('items', user, 'lib1')
check('items with resume', 'item=m1\tBig Buck\tMovie\t0\t120\t600' in out)
check('stream url', jf('stream', user, 'm1').strip() == url + '/Items/m1/Download?api_key=tok')
check('stream urls', jf('stream', user, 'm1', 'm2').split() == [url + '/Items/m1/Download?api_key=tok', url + '/Items/m2/Download?api_key=tok'])
check('resume', 'item=e1\tPilot\tEpisode\t0\t60\t600\tShow\t1' in jf('resume', user))
out = jf('image', user, 'm1').strip()
check('image cached', out == 'image=%s/jfcache/m1.img' % t and open(out[6:], 'rb').read() == JPEG)
n = len(seen)
check('image from cache', jf('image', user, 'm1').strip() == out and len(seen) == n)
check('no image', jf('image', user, 'm2').strip() == 'image=')
check('progress', jf('progress', user, 'm1', '42').startswith('OK') and any('"PositionTicks":420000000' in b for p, b, _ in seen))
check('bad item id', jf('items', user, '../x').startswith('ERR'))
check('logout', jf('logout', user).startswith('OK') and jf('status', user).count('signed=0') == 1)
srv.shutdown()
print('[PASS] media' if not fails else '[FAIL] %d checks' % fails)
sys.exit(1 if fails else 0)
