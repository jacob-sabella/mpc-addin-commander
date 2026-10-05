#!/usr/bin/env python3
"""The addin's server end to end: tests/fake_host (built with the addin's sources) loads tests/fake_plugin.so
through the hooked dlsym while this script talks to the WebSocket. Usage: test_ws.py <fake_host> <fake_plugin.so>.
No third-party packages: the WebSocket client below is the handshake plus masked text frames."""
import base64, hashlib, json, os, shutil, socket, struct, subprocess, sys, time, urllib.error, urllib.request

HOST, PLUGIN = sys.argv[1], os.path.abspath(sys.argv[2])
PORT = socket.socket(); PORT.bind(("127.0.0.1", 0)); PORT = PORT.getsockname()[1]
DIR = os.path.dirname(HOST)
CONF = os.path.join(DIR, "test_ws.conf")
SURFACE = os.path.join(DIR, "surface.bin")
SETTINGS = os.path.join(DIR, "MPC.settings")
XPJ = os.path.join(DIR, "Songs & Sketches", "Jam 1.xpj")
open(CONF, "w").write("bind=127.0.0.1\nport=%d\npoll_ms=20\ntext_ms=200\nmax_clients=24\nsurface=%s\nsettings=%s\n"
                      % (PORT, SURFACE, SETTINGS))
MIDI = os.path.join(DIR, "midi")
os.makedirs(MIDI, exist_ok=True)
for f in ("in.bin", "out.bin"):
    if os.path.exists(os.path.join(MIDI, f)):
        os.remove(os.path.join(MIDI, f))
for f in (SURFACE, SETTINGS):
    if os.path.exists(f):
        os.remove(f)


class Ws:
    def __init__(self, timeout=5):
        self.s = socket.create_connection(("127.0.0.1", PORT), timeout=timeout)
        key = base64.b64encode(os.urandom(16)).decode()
        self.s.sendall(("GET /ws HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                        "Sec-WebSocket-Key: %s\r\nSec-WebSocket-Version: 13\r\n\r\n" % key).encode())
        head = b""
        while b"\r\n\r\n" not in head:
            c = self.s.recv(1024)
            assert c, "closed during the handshake"
            head += c
        head, self.buf = head.split(b"\r\n\r\n", 1)
        assert head.startswith(b"HTTP/1.1 101"), head
        want = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        assert ("Sec-WebSocket-Accept: " + want).encode() in head, head

    def _read(self, n):
        while len(self.buf) < n:
            c = self.s.recv(65536)
            if not c:
                raise EOFError
            self.buf += c
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def recv_frame(self):
        b0, b1 = self._read(2)
        n = b1 & 0x7f
        if n == 126:
            n = struct.unpack(">H", self._read(2))[0]
        elif n == 127:
            n = struct.unpack(">Q", self._read(8))[0]
        assert not b1 & 0x80, "server frames are unmasked"
        return b0 & 0x0f, self._read(n)

    def send_frame(self, op, data=b""):
        mask = os.urandom(4)
        h = bytes([0x80 | op])
        if len(data) < 126:
            h += bytes([0x80 | len(data)])
        elif len(data) < 65536:
            h += bytes([0x80 | 126]) + struct.pack(">H", len(data))
        else:
            h += bytes([0x80 | 127]) + struct.pack(">Q", len(data))
        self.s.sendall(h + mask + bytes(c ^ mask[i & 3] for i, c in enumerate(data)))

    def send(self, obj):
        self.send_frame(1, json.dumps(obj).encode())

    def recv(self):
        while True:
            op, data = self.recv_frame()
            if op == 1:
                return json.loads(data)
            if op == 9:
                self.send_frame(10, data)
            elif op == 8:
                raise EOFError

    def wait(self, pred, timeout=3):
        """Messages until one satisfies pred (returned); the others are dropped."""
        end = time.time() + timeout
        while time.time() < end:
            m = self.recv()
            if pred(m):
                return m
        raise AssertionError("no message matched within %ss" % timeout)

    def close(self):
        self.s.close()


def fail(msg):
    print("FAIL " + msg)
    host.kill()
    sys.exit(1)


host = subprocess.Popen([HOST], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=sys.stderr, text=True,
                        env=dict(os.environ, MPC_COMMANDER_ADDIN_CONF=CONF, COMMANDER_FAKE_MIDI=MIDI), bufsize=1)


def cmd(line):
    host.stdin.write(line + "\n")
    host.stdin.flush()
    r = host.stdout.readline().strip()
    if r.startswith("error"):
        fail("host: " + line + " -> " + r)
    return r


for _ in range(100):
    try:
        socket.create_connection(("127.0.0.1", PORT), timeout=0.5).close()
        break
    except OSError:
        time.sleep(0.05)
else:
    fail("the server never listened")

info = json.load(urllib.request.urlopen("http://127.0.0.1:%d/info" % PORT))
assert info["protocol"] == 1 and info["plugins"] == 0, info
print("ok   /info:", info)

# an empty hello
ws = Ws()
hello = ws.recv()
assert hello["t"] == "hello" and hello["protocol"] == 1 and hello["poll_ms"] == 20, hello
assert ws.recv() == {"t": "plugins", "plugins": []}
FAKE_MIDI = hello["midi"] == {"client": 99}   # the sanitizer builds; the preload run has the real port or its error
assert FAKE_MIDI or "client" in hello["midi"] or "error" in hello["midi"], hello
tr = ws.recv()
assert tr == {"t": "transport", "playing": False, "recording": False, "tempo": None, "bar": 1, "beat": 1, "tick": 0, "source": ""}, tr
print("ok   hello, an empty plugin list and the stopped transport")

# load: plugin_added with the six params
n0 = int(cmd("load " + PLUGIN).split()[1])
added = ws.wait(lambda m: m["t"] == "plugin_added")
p = added["plugin"]
assert p["name"] == "Fake Synth" and p["vendor"] == "mpc-addin-commander tests" and p["uid"] == "46616b65", p
assert p["synth"] and p["so"] == PLUGIN and p["sample_rate"] == 44100 and not p["skin"], p
names = [q["name"] for q in p["params"]]
assert names == ["Cutoff", "Mode", "Sync", "Trigger", "Patch", "Level"], names
assert p["params"][0] == {"i": 0, "name": "Cutoff", "label": "Hz", "value": 0.5, "text": "10010"}, p["params"][0]
assert p["params"][1]["text"] == "Lowpass" and p["params"][5]["label"] == "dB", p["params"]
ID = p["id"]
print("ok   plugin_added: id %d, %d params" % (ID, len(p["params"])))
assert cmd("next %d" % n0) == "1", "dlsym(RTLD_NEXT) from a library loaded after the addin"
print("ok   RTLD_NEXT from the plugin skips the plugin itself")

# a second client sees it in its list; /plugins too
ws2 = Ws()
ws2.recv()
lst = ws2.recv()
assert [q["id"] for q in lst["plugins"]] == [ID], lst
doc = json.load(urllib.request.urlopen("http://127.0.0.1:%d/plugins" % PORT))
assert doc == lst, (doc, lst)
print("ok   the list for a late client and /plugins")

# the host's own setParameter reaches both clients as values with text
cmd("set %d 1 1.0" % n0)
for w in (ws, ws2):
    v = w.wait(lambda m: m["t"] == "values" and m["id"] == ID)
    assert [1, 1, "Highpass"] in v["v"], v
print("ok   a host-side set arrives as values")

# a client's set reaches the plugin (the host's getParameter) and the other client
ws.send({"t": "set", "id": ID, "i": 0, "value": 0.25})
v = ws2.wait(lambda m: m["t"] == "values" and m["id"] == ID and any(x[0] == 0 for x in m["v"]))
assert [0, 0.25, "5015"] in v["v"], v
assert cmd("get %d 0" % n0) == "0.25"
assert cmd("automated %d" % n0).split()[1:] == ["0", "0.25"]   # the host was told, so its screen follows
ws.send({"t": "set", "id": ID, "i": 0, "value": 7})     # clamped
ws2.wait(lambda m: m["t"] == "values" and m["id"] == ID and [0, 1, "20000"] in m["v"])
assert cmd("get %d 0" % n0) == "1"
print("ok   a client's set reaches the plugin and the other client, clamped")

# the engine changing a value itself (audioMasterAutomate from the plugin)
cmd("engine %d 5 0.1" % n0)
v = ws.wait(lambda m: m["t"] == "values" and m["id"] == ID and any(x[0] == 5 for x in m["v"]))
assert [5, 0.1, "-53.4"] in v["v"], v
print("ok   an engine-driven change arrives")

# a trigger: the plugin reports it back to 0 from its process call, with automate
ws.send({"t": "set", "id": ID, "i": 3, "value": 1})
v = ws.wait(lambda m: m["t"] == "values" and m["id"] == ID and [3, 0, "-"] in m["v"], timeout=3)
print("ok   a momentary parameter drops back")

# text re-read on the text timer and on UpdateDisplay: Patch's text changes
ws.send({"t": "set", "id": ID, "i": 4, "value": 1})
v = ws.wait(lambda m: m["t"] == "values" and m["id"] == ID and any(x[0] == 4 for x in m["v"]))
assert [4, 1, "Bass"] in v["v"], v
print("ok   text follows the value")

# errors
ws.send({"t": "set", "id": 999, "i": 0, "value": 0})
e = ws.wait(lambda m: m["t"] == "error")
assert "instance" in e["msg"], e
ws.send({"t": "nope"})
e = ws.wait(lambda m: m["t"] == "error")
ws.send({"t": "ping"})
ws.wait(lambda m: m["t"] == "pong")
ws.send({"t": "subscribe", "ids": [ID]})
ws.send({"t": "list"})
l = ws.wait(lambda m: m["t"] == "plugins")
assert len(l["plugins"]) == 1 and l["plugins"][0]["params"][4]["text"] == "Bass", l
print("ok   error, ping, subscribe, list")

# a second instance, then close both: plugin_removed, and the list empties
n1 = int(cmd("load " + PLUGIN).split()[1])
a2 = ws.wait(lambda m: m["t"] == "plugin_added")
ID2 = a2["plugin"]["id"]
assert ID2 != ID
cmd("close %d" % n0)
r = ws.wait(lambda m: m["t"] == "plugin_removed")
assert r["id"] == ID, r
cmd("close %d" % n1)
r = ws2.wait(lambda m: m["t"] == "plugin_removed" and m["id"] == ID2)
ws.send({"t": "list"})
assert ws.wait(lambda m: m["t"] == "plugins")["plugins"] == []
print("ok   a second instance, close both: plugin_removed, empty list")

# reload after dlclose (the module table refreshes the entry)
n2 = int(cmd("load " + PLUGIN).split()[1])
a3 = ws.wait(lambda m: m["t"] == "plugin_added")
assert a3["plugin"]["id"] > ID2
cmd("close %d" % n2)
ws.wait(lambda m: m["t"] == "plugin_removed")
print("ok   reload after the module was unloaded")

# /skin: a Plugin Skins folder next to the plugin (made here), path escapes refused
n3 = int(cmd("load " + PLUGIN).split()[1])
a4 = ws.wait(lambda m: m["t"] == "plugin_added")
ID4 = a4["plugin"]["id"]
assert not a4["plugin"]["skin"]
# /skin by name: no skin next to the .so, but "<vendor> - VST - <product>/Plugin Skins" beside its folder, as MPC finds it
byname = os.path.join(os.path.dirname(os.path.dirname(PLUGIN)), "mpc-addin-commander tests - VST - Fake Synth")
shutil.rmtree(byname, ignore_errors=True)
os.makedirs(os.path.join(byname, "Plugin Skins"))
open(os.path.join(byname, "Plugin Skins", "TUI.json"), "w").write('{"by": "name"}')
try:
    ws.send({"t": "list"})
    assert ws.wait(lambda m: m["t"] == "plugins")["plugins"][0]["skin"]
    r = urllib.request.urlopen("http://127.0.0.1:%d/skin/%d/TUI.json" % (PORT, ID4))
    assert r.read() == b'{"by": "name"}'
finally:
    shutil.rmtree(byname)
ws.send({"t": "list"})
assert not ws.wait(lambda m: m["t"] == "plugins")["plugins"][0]["skin"]
print("ok   a skin found by name beside the plugin's folder, as MPC finds it")

skin = os.path.join(os.path.dirname(PLUGIN), "Plugin Skins")
os.makedirs(os.path.join(skin, "sub"), exist_ok=True)
open(os.path.join(skin, "TUI.json"), "w").write('{"x": 1}')
open(os.path.join(skin, "sub", "a b.png"), "wb").write(b"\x89PNG fake")
secret = os.path.join(os.path.dirname(PLUGIN), "secret.txt")
open(secret, "w").write("no")
try:
    os.symlink(secret, os.path.join(skin, "link.txt"))
except OSError:
    pass


def get(path, headers={}):
    try:
        r = urllib.request.urlopen(urllib.request.Request("http://127.0.0.1:%d%s" % (PORT, path), headers=headers))
        return r.status, r.read(), dict(r.headers)
    except urllib.error.HTTPError as e:
        return e.code, e.read(), dict(e.headers)


st, body, h = get("/skin/%d/TUI.json" % ID4)
assert st == 200 and body == b'{"x": 1}' and h["Content-Type"] == "application/json" and h.get("ETag"), (st, h)
st2, _, _ = get("/skin/%d/TUI.json" % ID4, {"If-None-Match": h["ETag"]})
assert st2 == 304, st2
st, body, h = get("/skin/%d/sub/a%%20b.png" % ID4)
assert st == 200 and body == b"\x89PNG fake" and h["Content-Type"] == "image/png", (st, h)
for bad in ("/skin/%d/../fake_plugin.so" % ID4, "/skin/%d/link.txt" % ID4, "/skin/%d/" % ID4, "/skin/%d/sub" % ID4,
            "/skin/999/TUI.json", "/skin/%d/%%2e%%2e/secret.txt" % ID4):
    st, _, _ = get(bad)
    assert st == 404, (bad, st)
ws.send({"t": "list"})
assert ws.wait(lambda m: m["t"] == "plugins")["plugins"][0]["skin"]
print("ok   /skin serves the plugin's skin files, with ETag; escapes refused")

# ---- the DAW side ----
def midi_out():
    """What the addin sent on its port, as messages."""
    try:
        raw = open(os.path.join(MIDI, "out.bin"), "rb").read()
    except FileNotFoundError:
        return []
    out, k = [], 0
    while k < len(raw):
        out.append(list(raw[k + 1:k + 1 + raw[k]]))
        k += 1 + raw[k]
    return out


def midi_in(*msgs):
    """Messages from MPC into the addin's port."""
    with open(os.path.join(MIDI, "in.bin"), "ab") as f:
        for m in msgs:
            f.write(bytes([len(m)]) + bytes(m))


if FAKE_MIDI:
    ws.send({"t": "transport", "cmd": "play"})
    time.sleep(0.1)
    assert midi_out() == [[0xF0, 0x7F, 0x7F, 0x06, 0x02, 0xF7], [0xFA]], midi_out()
    ws.send({"t": "transport", "cmd": "record"})
    ws.send({"t": "transport", "cmd": "stop"})
    ws.send({"t": "transport", "cmd": "continue"})
    time.sleep(0.1)
    assert midi_out()[2:] == [[0xF0, 0x7F, 0x7F, 0x06, 0x06, 0xF7], [0xF0, 0x7F, 0x7F, 0x06, 0x02, 0xF7],
                              [0xF0, 0x7F, 0x7F, 0x06, 0x01, 0xF7], [0xFC], [0xF0, 0x7F, 0x7F, 0x06, 0x02, 0xF7], [0xFB]], midi_out()
    ws.send({"t": "transport", "cmd": "dance"})
    assert "cmd" in ws.wait(lambda m: m["t"] == "error")["msg"]
    print("ok   transport commands go out as MMC and MIDI real-time")

    ws.send({"t": "midi", "bytes": [0x90, 60, 100]})
    ws.send({"t": "midi", "bytes": [0xB0, 7, 127]})
    time.sleep(0.1)
    assert midi_out()[-2:] == [[0x90, 60, 100], [0xB0, 7, 127]], midi_out()
    ws.send({"t": "midi", "bytes": [0x90, 60]})
    assert ws.wait(lambda m: m["t"] == "error")
    ws.send({"t": "midi", "bytes": [300]})
    assert ws.wait(lambda m: m["t"] == "error")
    print("ok   midi from the app reaches the port; bad messages refused")

    # MPC clocks while stopped: a tempo, no movement (at 10 ms a clock: 250 bpm)
    for k in range(60):
        midi_in([0xF8])
        time.sleep(0.01)
    t = ws.wait(lambda m: m["t"] == "transport" and m["tempo"] is not None)
    assert not t["playing"] and t["bar"] == 1 and t["beat"] == 1 and 150 < t["tempo"] < 330, t
    print("ok   clock while stopped gives the tempo and doesn't move the position")
    # MPC starts, clocks for two beats (at 10 ms a clock: 250 bpm), sends a song position, then stops over MMC
    midi_in([0xFA])
    t = ws.wait(lambda m: m["t"] == "transport" and m["playing"])
    assert t["bar"] == 1 and t["beat"] == 1 and t["tick"] == 0 and t["source"] == "clock", t
    for k in range(48):
        midi_in([0xF8])
        time.sleep(0.01)
    t = ws.wait(lambda m: m["t"] == "transport" and m["beat"] == 3)
    assert t["bar"] == 1 and t["tick"] == 0 and t["playing"], t   # beats are pushed on the beat
    assert t["tempo"] and 150 < t["tempo"] < 330, t    # 250 bpm at 10 ms a clock, through the test's own sleep jitter
    # a pause in the clock isn't a slow tempo
    time.sleep(0.4)
    for k in range(60):
        midi_in([0xF8])
        time.sleep(0.01)
    t = ws.wait(lambda m: m["t"] == "transport" and m["bar"] == 2)   # 48 + 60 clocks: bar 2, beat 1
    assert 150 < t["tempo"] < 330, t
    # MMC locate: a time code position, turned into bars at the clock's tempo
    # MPC's own form has no sub-frame byte
    midi_in([0xF0, 0x7F, 0x00, 0x06, 0x44, 0x06, 0x01, 0x20 | 0, 0, 3, 0, 0xF7])   # 25 fps, 0:00:03.00
    t = ws.wait(lambda m: m["t"] == "transport" and m["bar"] != 2)
    beats = 3 * t["tempo"] / 60   # at the tempo the addin had then
    assert abs((t["bar"] - 1) * 4 + (t["beat"] - 1) + t["tick"] / 960 - beats) < 0.05, (t, beats)
    midi_in([0xF0, 0x7F, 0x00, 0x06, 0x44, 0x06, 0x01, 0, 0, 0, 0, 0, 0xF7])
    t = ws.wait(lambda m: m["t"] == "transport" and m["bar"] == 1 and m["beat"] == 1 and m["tick"] == 0)
    print("ok   a clock pause keeps the tempo; MMC locate sets the position")
    midi_in([0xF2, 0, 1])               # 128 sixteenths = bar 9
    t = ws.wait(lambda m: m["t"] == "transport" and m["bar"] == 9)
    assert t["beat"] == 1 and t["tick"] == 0, t
    midi_in([0xF0, 0x7F, 0x7F, 0x06, 0x01, 0xF7])
    t = ws.wait(lambda m: m["t"] == "transport" and not m["playing"])
    assert t["source"] == "mmc" and not t["recording"], t
    midi_in([0xF0, 0x7F, 0x7F, 0x06, 0x06, 0xF7], [0xF0, 0x7F, 0x7F, 0x06, 0x02, 0xF7])
    t = ws.wait(lambda m: m["t"] == "transport" and m["playing"] and m["recording"])
    midi_in([0xFC])
    ws.wait(lambda m: m["t"] == "transport" and not m["playing"])
    print("ok   MPC's start, clock, song position and MMC become transport state")

    # anything else from MPC's port is handed to the app
    midi_in([0x91, 64, 90], [0xF0, 0x43, 0x10, 0x4C, 0x00, 0x00, 0x7E, 0x00, 0xF7])
    m = ws.wait(lambda m: m["t"] == "midi_in" and m["bytes"][0] == 0x91)
    assert m["bytes"] == [0x91, 64, 90] and isinstance(m["ms"], int), m
    m = ws.wait(lambda m: m["t"] == "midi_in" and m["bytes"][0] == 0xF0)
    assert m["bytes"] == [0xF0, 0x43, 0x10, 0x4C, 0x00, 0x00, 0x7E, 0x00, 0xF7], m
    # a late client gets the current transport state right after the plugin list
    w = Ws(); w.recv(); w.recv()
    assert w.recv()["t"] == "transport"
    w.close()
    print("ok   other MIDI from MPC arrives as midi_in; late clients get the transport state")

# the control-surface injector file: appended when it exists, refused when it doesn't
ws.send({"t": "surface", "bytes": [0x90, 0x7B, 0x7F, 0x90, 0x7B, 0x00]})
assert "surface" in ws.wait(lambda m: m["t"] == "error")["msg"]
open(SURFACE, "wb").close()
ws.send({"t": "surface", "bytes": [0x90, 0x7B, 0x7F, 0x90, 0x7B, 0x00]})
ws.send({"t": "surface", "bytes": [0x9F, 0x3E, 0x00]})
ws.send({"t": "ping"})
ws.wait(lambda m: m["t"] == "pong")
assert open(SURFACE, "rb").read() == bytes([0x90, 0x7B, 0x7F, 0x90, 0x7B, 0x00, 0x9F, 0x3E, 0x00])
print("ok   surface bytes are appended to the injector file")

# a project snapshot: the most recent project in the settings, gzip over five header lines and JSON
import gzip
ws.send({"t": "project"})
assert "settings" in ws.wait(lambda m: m["t"] == "error")["msg"]
os.makedirs(os.path.dirname(XPJ), exist_ok=True)
body = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures", "project.json"), "rb").read()
open(XPJ, "wb").write(gzip.compress(b"ACVS\n3.9.1.2\nSerialisableProjectData\njson\nLinux\n" + body))
open(SETTINGS, "w").write('<?xml version="1.0"?>\n<PROPERTIES>\n  <VALUE name="recentProject2" val="/nowhere/Old.xpj"/>\n'
                          '  <VALUE name="recentProject1" val="%s"/>\n</PROPERTIES>\n' % XPJ.replace("&", "&amp;"))
ws.send({"t": "project"})
pr = ws.wait(lambda m: m["t"] == "project")
assert pr["path"] == XPJ and pr["name"] == "Jam 1" and pr["key"] == "F# Minor" and pr["version"] == 28, pr
assert pr["tempo"] == 93.5 and pr["master_tempo"] == 128 and not pr["master_tempo_enabled"] and pr["current_track"] == 1, pr
assert pr["sequence"] == {"index": 1, "name": "Verse", "tempo": 93.5, "tempo_enabled": True, "bars": 8, "loop": True,
                          "loop_start": 4, "loop_end": 8, "beats_per_bar": 3, "beat_length": 960}, pr["sequence"]
tr = pr["tracks"]
assert [t["name"] for t in tr] == ["Drums", "Keys & Pads", "Muted Seq", "Return 1", "Submix 1", "Out 1/2", "Mystery"], tr
assert [t["type"] for t in tr] == ["drum", "plugin", "plugin", "return", "submix", "output", "audio"], tr
assert tr[1]["plugin"] == {"name": "Chordsmith", "vendor": "jacob-sabella", "format": "VST", "file": "/storage/Synths/x/x.so",
                           "preset": "Lydian Pad"} and tr[1]["solo"] and tr[1]["record_arm"] and tr[1]["volume"] == 0.45, tr[1]
assert tr[2]["mute"] and tr[2]["track_mute"] and tr[2]["plugin"]["name"] == "Fake Synth" and tr[0]["plugin"] is None, tr
assert tr[6]["kind"] == 6 and tr[6]["volume"] == 0.1, tr[6]
W0 = "/data/tracks[0]/program/drum/instruments[1]/mixable/inserts/effects[0]/plugin/plugin"
W3 = "/data/tracks[3]/program/mixable/inserts/effects[0]/plugin/plugin"
assert pr["stock"] == [
    {"where": W0, "name": "Fake Comp", "vendor": "Test Vendor", "preset": "Init", "state": "8.zzzz...."},
    {"where": W3, "name": "Fake Verb", "vendor": "Test Vendor", "preset": "Room", "state": "12.ABCDEFGHIJK+"},
], pr["stock"]   # a VST insert and a state that isn't JUCE base64 are left out
doc = json.load(urllib.request.urlopen("http://127.0.0.1:%d/project" % PORT))
assert doc == pr, doc
assert pr["mtime"] > 0
open(XPJ, "wb").write(b"not gzip at all")
st, body, _ = get("/project")
assert st == 404 and b"project file" in body, (st, body)   # zlib reads a plain file as is: no header
print("ok   the project snapshot: tempo, sequence, tracks and plugins; a broken file is an error")

if os.environ.get("STOCK_TEST"):   # Akai's own skins, read where they are installed (here the test root)
    F = "/stock/Test%20Vendor%20-%20MPC%20-%20Fake%20Verb/"
    st, body, h = get(F + "TUI.json")
    assert st == 200 and body == b'{"pageData":{}}' and h["ETag"], (st, body)
    st2, _, _ = get(F + "TUI.json", {"If-None-Match": h["ETag"]})
    assert st2 == 304, st2
    st, body, _ = get(F + "knob.png")
    assert st == 200 and body == b"png", (st, body)
    for bad in (F + "../../secret.txt", F + "%2e%2e/%2e%2e/secret.txt", "/stock/Test%20Vendor%20-%20MPC%20-%20Nope/TUI.json",
                "/stock/../stock/Test%20Vendor%20-%20MPC%20-%20Fake%20Verb/TUI.json", "/stock/secret.txt", "/stock/",
                "/stock/Test%20Vendor%20-%20VST%20-%20Fake%20Verb/TUI.json", F):
        st, _, _ = get(bad)
        assert st == 404, (bad, st)
    print("ok   stock skins: files with an ETag; no other folder, no escape")

# many clients at once, all fed
many = [Ws() for _ in range(20)]
for w in many:
    w.recv(); w.recv()
cmd("set %d 2 1" % n3)
for w in many:
    w.wait(lambda m: m["t"] == "values" and [2, 1, "On"] in m["v"])
for w in many:
    w.close()
print("ok   20 clients at once")

# a client that goes away mid-stream does not stop the others
w = Ws(); w.recv(); w.recv()
w.s.close()
cmd("set %d 2 0" % n3)
ws.wait(lambda m: m["t"] == "values" and [2, 0, "Off"] in m["v"])
print("ok   a vanished client is dropped")

ws.send_frame(8, b"\x03\xe8")
op, _ = ws.recv_frame()
assert op == 8, op
ws2.close()
cmd("quit")
host.wait(timeout=5)
print("ws tests passed")
