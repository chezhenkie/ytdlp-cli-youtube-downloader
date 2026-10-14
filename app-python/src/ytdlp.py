#!/usr/bin/env python3
"""Interactive CLI and quick launcher around yt-dlp.

Menu mode:   python app-python/src/ytdlp.py
Quick mode:  python app-python/src/ytdlp.py <url> [-a] [-f mp4] [-r 1080] [--subs]
Checks:      python app-python/src/ytdlp.py --check | --selftest

Stdlib only. Windows / Linux / macOS. Defaults persist in app-python/config/settings.json.
"""
import argparse
import hashlib
import json
import math
import os
import random
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import unicodedata
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

VERSION = "1.4.1"
ROOT = Path(__file__).resolve().parent.parent
CONFIG_PATH = ROOT / "config" / "settings.json"
BIN_DIR = ROOT / "bin"
WIDTH = 64

A_AMBER = "\033[38;5;214m"
A_LBLUE = "\033[38;5;117m"
A_GREY = "\033[38;5;250m"
A_DARKGREY = "\033[38;5;244m"
A_MINT = "\033[38;5;121m"
A_PURPLE = "\033[38;5;176m"
A_RESET = "\033[0m"
CLEAR = "\033[2J\033[H"
USE_COLOR = sys.stdout.isatty()


def enable_vt_output():
    """Windows consoles need ENABLE_VIRTUAL_TERMINAL_PROCESSING for ANSI."""
    if os.name != "nt":
        return
    try:
        import ctypes
        k = ctypes.windll.kernel32
        h = k.GetStdHandle(-11)
        mode = ctypes.c_uint32()
        if k.GetConsoleMode(h, ctypes.byref(mode)):
            k.SetConsoleMode(h, mode.value | 0x0004)
    except Exception:
        pass


def c(code, text):
    return code + text + A_RESET if USE_COLOR else text


SIZE_RE = re.compile(r"^\d+(\.\d+)?[KMGT]?i?B$")
SPEED_RE = re.compile(r"^\d+(\.\d+)?[KMGT]?i?B/s$")
TIME_RE = re.compile(r"^\d{1,2}:\d{2}(:\d{2})?$")
PLAIN_WORDS = ("of", "at", "in", "ETA")


def one_dec_num(word):
    """Numbers print with one decimal, truncated: yt-dlp emits two
    (pinned source _utils.py, format_bytes -> %.2f%sB) and an extra digit
    buys nothing on a progress line. 9.94MiB/s -> 9.9MiB/s forever."""
    m = re.match(r"^(~?\d+)\.(\d+)([A-Za-z/%]+)$", word)
    if not m:
        return word
    return "%s.%s%s" % (m.group(1), m.group(2)[:1], m.group(3))


def pad_speed(word):
    """The speed keeps a zero-padded three digit integer part, so the
    digit crossings the user reported (9.9 -> 10.1 MiB/s) never shift the
    rest of the line: 009.3MiB/s. Three integer digits cover the whole
    range below yt-dlp's Global iB switch at 1024, so the field stays
    exactly ten characters wide - the width yt-dlp itself pads its speed
    to (pinned source common.py format_speed: >10s)."""
    if not SPEED_RE.match(word):
        return word
    m = re.match(r"^(\d+)(\.\d+)?([A-Za-z/]+)$", one_dec_num(word))
    if not m:
        return word
    return m.group(1).zfill(3) + (m.group(2) or "") + m.group(3)


def color_progress(body):
    """Color one [download] body: four numbers, four colors.

    Percent amber 214, file size mint green 121, speed light blue 117, ETA
    desaturated purple 176; the plain words ("of", "at", "in", "ETA") are
    grey 250. Numbers shrink to one decimal; the speed is zero-padded to
    three integer digits so its width never changes. Pure function,
    testable without a tty; off a tty the caller skips it entirely."""
    out = []
    for word in body.split(" "):
        if "%" in word:
            out.append(A_AMBER + one_dec_num(word) + A_RESET)
        elif SIZE_RE.match(word) or (word.startswith("~")
                                     and SIZE_RE.match(word[1:])):
            out.append(A_MINT + one_dec_num(word) + A_RESET)
        elif SPEED_RE.match(word):
            out.append(A_LBLUE + pad_speed(word) + A_RESET)
        elif TIME_RE.match(word):
            out.append(A_PURPLE + word + A_RESET)
        elif word in PLAIN_WORDS:
            out.append(A_GREY + word + A_RESET)
        else:
            out.append(word)
    return " ".join(out)


class GoBack(Exception):
    """Escape pressed inside a submenu: return to the main menu."""


HISTORY = []
HISTORY_MAX = 100


class LineEdit:
    """Line buffer with echo and history recall for read_esc.

    Up/down walk the session history of submitted entries (urls and menu
    numbers alike); the half-typed line is kept as the draft and comes back
    when walking down past the newest entry. Left/right arrows are consumed
    without effect - the reader does no cursor-position editing."""

    def __init__(self, prompt):
        self.prompt = prompt
        self.buf = []
        self.idx = len(HISTORY)
        self.draft = ""

    def text(self):
        return "".join(self.buf)

    def add(self, ch):
        self.buf.append(ch)
        sys.stdout.write(ch)
        sys.stdout.flush()

    def back(self):
        if self.buf:
            self.buf.pop()
            sys.stdout.write("\b \b")
            sys.stdout.flush()

    def _show(self, text):
        self.buf = list(text)
        sys.stdout.write("\r\033[K%s%s" % (self.prompt, text))
        sys.stdout.flush()

    def remember(self):
        """Record a submitted line (non-empty) for later recall."""
        line = self.text()
        if line.strip():
            HISTORY.append(line)
            del HISTORY[:-HISTORY_MAX]

    def recall(self, older):
        """older True steps to a previous entry, False to a newer one."""
        if not HISTORY:
            return
        if older:
            if self.idx == len(HISTORY):
                self.draft = self.text()
            if self.idx > 0:
                self.idx -= 1
                self._show(HISTORY[self.idx])
        elif self.idx < len(HISTORY) - 1:
            self.idx += 1
            self._show(HISTORY[self.idx])
        elif self.idx == len(HISTORY) - 1:
            self.idx = len(HISTORY)
            self._show(self.draft)


def _esc_more(fd, timeout=0.05):
    """Next byte of an in-flight ESC sequence, or None if nothing follows.

    Distinguishes a lone escape (abort) from arrow keys, which arrive on
    posix terminals as ESC [ A / ESC [ B."""
    import select
    try:
        ready, _, _ = select.select([fd], [], [], timeout)
    except (OSError, ValueError):
        return None
    if not ready:
        return None
    try:
        return os.read(fd, 1).decode("utf-8", "replace")
    except OSError:
        return None


def _csi_final(fd):
    """Final letter of a CSI key sequence; parameter bytes are swallowed."""
    while True:
        ch = _esc_more(fd)
        if ch is None or ord(ch) >= 0x40:
            return ch


def block_under_input(block):
    """True when a reference block can be painted under a live input line.

    Needs colour (the cursor-up escape), a real console, and room: a block
    wider than the window would wrap and a block taller than the window
    would scroll the prompt off the top, and in both cases counting rows to
    move the caret back would land on the wrong line. False means the caller
    paints the block above the prompt instead, where it is always safe."""
    if not USE_COLOR or not sys.stdin.isatty():
        return False
    try:
        size = os.get_terminal_size()
    except Exception:
        return False
    return (max(len(l) for l in block) + 1 < size.columns
            and len(block) + 2 < size.lines)


def read_esc(prompt="", below=None):
    """One input line; returns None if Escape was pressed.

    Raw console reader so escape is detectable. Falls back to input() when
    stdin is not a tty (pipes, tests). `below` paints reference lines under
    the prompt and moves the caret back to the prompt line, so a list can be
    read while typing with the input above it."""
    if not sys.stdin.isatty():
        for line in (below or ()):
            print(line)
        return input(prompt)
    if prompt:
        sys.stdout.write(prompt)
        sys.stdout.flush()
    if below:
        sys.stdout.write("\n" + "\n".join(below) + "\n")
        sys.stdout.write("\033[%dA" % (len(below) + 1))
        sys.stdout.flush()
    if os.name == "nt":
        import msvcrt
        ed = LineEdit(prompt)
        while True:
            ch = msvcrt.getwch()
            if ch == "\r" or ch == "\n":
                sys.stdout.write("\n")
                sys.stdout.flush()
                ed.remember()
                return ed.text()
            if ch == "\x1b":
                sys.stdout.write("\r\033[K")
                sys.stdout.flush()
                return None
            if ch in ("\x08", "\x7f"):
                ed.back()
                continue
            if ch in ("\x00", "\xe0"):
                # extended key: swallow the scan code, arrows edit history
                code = msvcrt.getwch()
                if code == "H":
                    ed.recall(True)
                elif code == "P":
                    ed.recall(False)
                continue
            if ch == "\x03":
                raise KeyboardInterrupt
            ed.add(ch)
    import termios
    import tty
    fd = sys.stdin.fileno()
    old = termios.tcgetattr(fd)
    ed = LineEdit(prompt)
    try:
        tty.setraw(fd)
        while True:
            ch = sys.stdin.read(1)
            if ch in ("\r", "\n"):
                sys.stdout.write("\n")
                sys.stdout.flush()
                ed.remember()
                return ed.text()
            if ch == "\x1b":
                nxt = _esc_more(fd)
                if nxt is None:
                    sys.stdout.write("\r\033[K")
                    sys.stdout.flush()
                    return None
                if nxt == "[":
                    final = _csi_final(fd)
                    if final == "A":
                        ed.recall(True)
                    elif final == "B":
                        ed.recall(False)
                continue
            if ch in ("\x7f", "\x08"):
                ed.back()
                continue
            if ch == "\x03":
                raise KeyboardInterrupt
            if ch >= " ":
                ed.add(ch)
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old)

URL_RE = re.compile(r"https?://\S+")
ABS_RE = re.compile(r"^([A-Za-z]:[\\/]|[\\/])")

DEFAULTS = {
    "settings_version": 1,
    "yt_dlp_path": "",
    "ffmpeg_path": "",
    "auto_update": True,
    "auto_update_last_check": 0,
    "type": "video",
    "video_container": "mp4",
    "video_resolution": "1080",
    "audio_format": "mp3",
    "audio_quality": "0",
    # empty = auto-resolve: OS desktop, fallback OS download folder
    "output_dir": "",
    "filename_template": "%(title)s",
    "subtitles": True,
    "subtitle_langs": "en",
    "playlist": False,
    "playlist_max_items": 0,
}

RESOLUTIONS = ["2160", "1440", "1080", "720", "480", "360", "best"]
CONTAINERS = ["mp4", "mkv", "webm", "best"]
AUDIO_FORMATS = ["mp3", "m4a", "opus", "wav", "best"]
TYPES = ["video", "video no audio", "audio only"]
TEMPLATE_PRESETS = [
    "%(title)s",
    "%(title)s %(upload_date)s",
    "%(title)s %(timestamp>%Y-%m-%d %H-%M)s",
    "%(title)s %(id)s",
]
# every language code the menu offers and validates against, 191 in all.
# the 190 two-letter primary subtags of the IANA BCP 47 language subtag
# registry (iana.org, fetched 2026-10-13), which is ISO 639-1 plus the
# grandfathered aliases youtube still serves (in, iw, ji, mo, sh), plus fil:
# that one came from the pinned yt-dlp source (_SUPPORTED_LANG_CODES in
# yt_dlp/extractor/youtube/_base.py) because filipino has no two-letter form.
# it is a reference and not a gate: subtitle codes arrive from
# captionTracks[].languageCode per video and youtube publishes no full list,
# so a well formed code missing here is kept with a note, never refused.
KNOWN_LANG_CODES = frozenset("""
    aa  ab  ae  af  ak  am  an  ar  as  av  ay  az  ba  be  bg  bh
    bi  bm  bn  bo  br  bs  ca  ce  ch  co  cr  cs  cu  cv  cy  da
    de  dv  dz  ee  el  en  eo  es  et  eu  fa  ff  fi  fil  fj  fo
    fr  fy  ga  gd  gl  gn  gu  gv  ha  he  hi  ho  hr  ht  hu  hy
    hz  ia  id  ie  ig  ii  ik  in  io  is  it  iu  iw  ja  ji  jv
    jw  ka  kg  ki  kj  kk  kl  km  kn  ko  kr  ks  ku  kv  kw  ky
    la  lb  lg  li  ln  lo  lt  lu  lv  mg  mh  mi  mk  ml  mn  mo
    mr  ms  mt  my  na  nb  nd  ne  ng  nl  nn  no  nr  nv  ny  oc
    oj  om  or  os  pa  pi  pl  ps  pt  qu  rm  rn  ro  ru  rw  sa
    sc  sd  se  sg  sh  si  sk  sl  sm  sn  so  sq  sr  ss  st  su
    sv  sw  ta  te  tg  th  ti  tk  tl  tn  to  tr  ts  tt  tw  ty
    ug  uk  ur  uz  ve  vi  vo  wa  wo  xh  yi  yo  za  zh  zu
""".split())
# a language code with optional subtags: en, fil, en-GB, sr-Latn, es-419, zh-Hans
LANG_CODE_RE = re.compile(r"^[a-z]{2,3}(?:-[a-z0-9]{2,8})*$")
PLATFORM_BIN = {"windows": "yt-dlp.exe", "linux": "yt-dlp", "macos": "yt-dlp"}

# where a lean install fetches ffmpeg from, per platform, with the file that
# carries the published hash. gyan.dev publishes only windows binaries and its
# zip is the release "essentials" build; BtbN/FFmpeg-Builds serves the linux
# tarball under a stable "latest" tag and publishes one checksums.sha256 for
# every asset in the release. macos has no source here: no free static build is
# published for it, so that platform gets instructions instead of a download.
# the essentials build was probed 2026-10-13: ffmpeg 9.0.2, 114768076 bytes,
# sha256 60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba,
# --enable-gpl --enable-version3, with libmp3lame, aac, libopus, pcm_s16le,
# the mp4/matroska/ipod muxers and the subrip/ass/webvtt demuxers all present,
# i.e. everything merge, mp3/m4a/opus/wav conversion and srt subtitles need.
FFMPEG_SOURCES = {
    "windows": ("https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip",
                "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip.sha256",
                "ffmpeg.exe", "zip"),
    "linux": ("https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/"
              "ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz",
              "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/checksums.sha256",
              "ffmpeg", "tar.xz"),
}
# the CLI's own source: the raw file on the default branch. a version check
# costs one GET, and the version is read
# from the same bytes that replace the file, so there is no second request that
# could disagree with it.
CLI_UPDATE_URL = ("https://raw.githubusercontent.com/chezhenkie/"
                  "ytdlp-cli-youtube-downloader/main/app-python/src/ytdlp.py")
CLI_SELF_PATH = Path(__file__).resolve()
UPDATE_TAG = "https://www.gyan.dev/ffmpeg/ and github.com/BtbN/FFmpeg-Builds"
DL_TIMEOUT = 1800
# audit F3 (2026-10-14): the daily auto-update check inherited the fetch
# helper's 60 s timeout on the main thread, so a black-holed connection froze
# the menu for a minute before the first keystroke was read. Owner-pinned:
# 6 s for the non-forced check, 60 s stays for the forced paths.
CHECK_TIMEOUT = 6



def platform_name():
    if os.name == "nt":
        return "windows"
    if sys.platform.startswith("linux"):
        return "linux"
    if sys.platform == "darwin":
        return "macos"
    return os.name


# ---------------------------------------------------------------- settings

# yt-dlp --audio-quality SPEC (pinned source yt_dlp/options.py, read
# 2026-10-14): "a value between 0 (best) and 10 (worst) for VBR or a specific
# bitrate like 128K". The accepted forms are therefore a plain number or
# "<number>K"; anything else is a hand-edit mistake, not a setting.
AUDIO_QUALITY_RE = re.compile(r"^\d+(?:\.\d+)?K?$", re.IGNORECASE)


def coerce_settings(s):
    """audit F1/F2 (2026-10-14): settings.json is user-editable and its
    values flow raw into int() and yt-dlp argv. Coerce at this one boundary
    so a mistyped value can never raise and never reaches argv unvalidated.
    Coercion happens in memory only - the file is never rewritten (user
    settings are theirs). Returns the notes."""
    d = DEFAULTS
    notes = []

    def keep_default(key, why):
        notes.append("setting %s %s - keeping default %r" % (key, why, d[key]))
        s[key] = json.loads(json.dumps(d[key]))

    raw = s.get("playlist_max_items")
    if isinstance(raw, bool) or not isinstance(raw, int):
        try:
            s["playlist_max_items"] = int(str(raw).strip())
        except (TypeError, ValueError):
            keep_default("playlist_max_items", "is not a number")
    for key, vocab in (("type", TYPES),
                       ("video_container", CONTAINERS),
                       ("video_resolution", RESOLUTIONS),
                       ("audio_format", AUDIO_FORMATS)):
        if s.get(key) not in vocab:
            keep_default(key, "is not one of " + "|".join(vocab))
    q = str(s.get("audio_quality", "")).strip()
    if not AUDIO_QUALITY_RE.match(q):
        keep_default("audio_quality", "is not a quality value")
    else:
        s["audio_quality"] = q or d["audio_quality"]
    return notes


def load_settings():
    s = json.loads(json.dumps(DEFAULTS))
    if CONFIG_PATH.exists():
        try:
            data = json.loads(CONFIG_PATH.read_text(encoding="utf-8"))
        except Exception as exc:
            print("settings unreadable (%s), using defaults" % exc)
            return s
        for key in data:
            if key in s:
                s[key] = data[key]
        if "type" not in data and data.get("audio_only"):
            s["type"] = "audio only"
    for note in coerce_settings(s):
        print("  %s" % c(A_AMBER, "note: " + note))
    return s


def save_settings(s):
    """Write settings.json, then swap it in.

    newline="\\n" on purpose: the default text mode would translate to CRLF on
    windows, and the tracked file is LF, so every save would leave git
    reporting the user's settings as modified."""
    data = json.dumps(s, indent=2, sort_keys=True) + "\n"
    try:
        if CONFIG_PATH.read_text(encoding="utf-8") == data:
            return
    except OSError:
        pass
    CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
    tmp = CONFIG_PATH.with_suffix(".tmp")
    with open(tmp, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(data)
    tmp.replace(CONFIG_PATH)


def onoff(flag):
    return "on" if flag else "off"


def _win_shell_folder(key):
    """Path from the Windows Shell Folders registry key (Desktop, Downloads),
    or None. The key follows folder redirection (OneDrive, moved profiles)."""
    try:
        import winreg
    except ImportError:
        return None
    try:
        with winreg.OpenKey(
                winreg.HKEY_CURRENT_USER,
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders") as k:
            val, _ = winreg.QueryValueEx(k, key)
    except OSError:
        return None
    return os.path.expandvars(val) if val else None


def _home_dir(name):
    return str(Path.home() / name)


def _xdg_dir(key):
    """Value of a key in ~/.config/user-dirs.dirs, expanded, or None."""
    cfg = Path.home() / ".config" / "user-dirs.dirs"
    try:
        txt = cfg.read_text(encoding="utf-8")
    except OSError:
        return None
    m = re.search(r'^\s*%s\s*=\s*"([^"]*)"' % key, txt, re.M)
    if not m:
        return None
    val = os.path.expandvars(m.group(1).replace("$HOME", str(Path.home())))
    return val


_outdir_cache = []


def auto_outdir():
    """Default output folder: the OS desktop, fallback the OS download folder.
    Returns None when neither exists (caller uses downloads/ at the project
    root). Desktop first on windows (Shell Folders key) and XDG on unix, with
    ~/Desktop and ~/Downloads as the last candidates; every candidate is
    gated on os.path.isdir so a redirected-but-missing folder falls through.

    Memoized for the process (audit F6, 2026-10-14): every menu repaint and
    several download-path resolutions re-ran the registry / user-dirs reads.
    The inputs (registry, user-dirs file, home) cannot change within a
    process, so one lookup answers for all of them."""
    if not _outdir_cache:
        _outdir_cache.append(_auto_outdir_lookup())
    return _outdir_cache[0]


def _auto_outdir_lookup():
    if os.name == "nt":
        cands = [
            _win_shell_folder("Desktop"),
            _win_shell_folder("{374DE290-123F-4565-9164-39C4925E467B}"),
        ]
    else:
        cands = [
            _xdg_dir("XDG_DESKTOP_DIR"),
            _xdg_dir("XDG_DOWNLOAD_DIR"),
        ]
    cands.append(_home_dir("Desktop"))
    cands.append(_home_dir("Downloads"))
    for c in cands:
        if c and os.path.isdir(c):
            return c
    return None


def resolve_outdir(s):
    """Output folder for downloads: the saved folder when the user set one,
    else the OS default (Desktop, fallback the OS download folder), else
    downloads/ under the project root."""
    out = (s.get("output_dir") or "").strip()
    if not out:
        auto = auto_outdir()
        out = auto if auto else "downloads"
    p = Path(out)
    return p if p.is_absolute() else (ROOT / p)


# ---------------------------------------------------------------- binary lookup


def find_yt_dlp(s):
    cands = []
    if s.get("yt_dlp_path"):
        cands.append(Path(s["yt_dlp_path"]))
    cands.append(BIN_DIR / PLATFORM_BIN.get(platform_name(), "yt-dlp"))
    found = shutil.which("yt-dlp") or shutil.which("yt-dlp.exe")
    if found:
        cands.append(Path(found))
    for c in cands:
        if c and c.is_file():
            return c
    return None


def find_ffmpeg_dir(s):
    if s.get("ffmpeg_path"):
        p = Path(s["ffmpeg_path"])
        if p.is_dir():
            return p
        if p.parent.is_dir():
            return p.parent
    binname = "ffmpeg.exe" if os.name == "nt" else "ffmpeg"
    if (BIN_DIR / binname).is_file():
        return BIN_DIR
    if (BIN_DIR / "ffmpeg").is_dir():
        return BIN_DIR
    hit = shutil.which("ffmpeg")
    if hit:
        return Path(hit).parent
    return None


_VERSION_CACHE = {}


def probe_version(binary):
    """Version line and rc for a binary. It is asked the double-dash way first
    because yt-dlp wants that spelling, then the single-dash way, because
    ffmpeg 9 rejects "--version" outright - it prints its banner to stderr and
    exits 0xABAFB008 - and answers "-version" on stdout with rc 0. ffmpeg also
    writes to stderr where yt-dlp writes to stdout, so both streams are read
    and the first attempt with rc 0 wins.

    Successful answers are memoized per binary path+mtime for the process
    (audit F4, 2026-10-14): the preflight re-probed on every download, one
    extra process spawn (~0.85 s, audit timing) for a version the session
    already saw. Failures are not memoized - a retry must stay a retry. A
    self-update or a replaced binary changes mtime, so the cache cannot
    answer for a binary it did not see."""
    try:
        key = (str(binary), os.path.getmtime(binary))
    except OSError:
        key = (str(binary), None)
    if key in _VERSION_CACHE:
        return _VERSION_CACHE[key]
    last = ("no version output", -1)
    for flag in ("--version", "-version"):
        try:
            r = subprocess.run([str(binary), flag], capture_output=True,
                               text=True, timeout=60)
        except Exception as exc:
            return "error: %s" % exc, -1
        blob = ((r.stdout or "") + (r.stderr or "")).strip()
        line = blob.splitlines()[0].strip() if blob else ""
        if r.returncode == 0 and line:
            _VERSION_CACHE[key] = (line, 0)
            return line, 0
        if line:
            last = (line, r.returncode)
    return last


# ---------------------------------------------------------------- arg builder

def clean_url(text):
    m = URL_RE.search(text or "")
    if not m:
        return None
    return m.group(0).rstrip(".,;:!?)]\"'")


def js_runtime_args():
    """Prefer a JS runtime when one is on PATH: silences yt-dlp's
    deprecation warning and restores format coverage. Empty when absent."""
    if shutil.which("node"):
        return ["--js-runtimes", "node"]
    return []


def sub_lang_patterns(langs):
    """'en,nl' -> 'en(-.*)?,nl(-.*)?' so en, en-GB, en-US and en-orig all
    match. Entries that already are patterns (* ? ( [) pass through."""
    out = []
    for part in str(langs or "").split(","):
        code = part.strip()
        if not code:
            continue
        if any(ch in code for ch in "*?(["):
            out.append(code)
        else:
            out.append(code + "(-.*)?")
    return ",".join(out) or "en(-.*)?"


def parse_lang_input(raw):
    """Split a typed language list into what to keep and what to say about it.

    Returns (kept, bad, unknown). bad is anything that cannot be a language
    code at all (a bare digit, junk); unknown is a well formed code that
    KNOWN_LANG_CODES does not carry, kept anyway because subtitle codes come
    from youtube per video. Tokens carrying a wildcard pass through, matching
    what sub_lang_patterns already accepts."""
    kept = []
    bad = []
    unknown = []
    for part in str(raw or "").split(","):
        code = part.strip().lower()
        if not code:
            continue
        wild = any(ch in code for ch in "*?([")
        if wild or LANG_CODE_RE.match(code):
            kept.append(code)
            if not wild and code.split("-")[0] not in KNOWN_LANG_CODES:
                unknown.append(code)
        else:
            bad.append(code)
    return kept, bad, unknown


def output_args(s):
    """Shared -o/-P pair so the video run and the subtitle run agree on
    filenames."""
    template = s["filename_template"] or DEFAULTS["filename_template"]
    opts = ["-o", template]
    if not ABS_RE.match(template):
        opts += ["-P", "home:" + str(resolve_outdir(s))]
    return opts


def build_args(s, url, ffdir):
    """Build yt-dlp argv (binary first, url last). ffdir is explicit: Path or
    None for "no ffmpeg" (forces single-file / no-convert branches).
    Subtitles are NOT part of this run - build_sub_args handles them after
    the video, so a rate-limited subtitle fetch cannot block the video."""
    opts = []
    opts += output_args(s)
    opts += ["--retries", "5"]
    opts += js_runtime_args()

    kind = s.get("type", "video")
    if kind == "audio only":
        if ffdir:
            opts += ["-f", "bestaudio/best",
                     "-x",
                     "--audio-format", s["audio_format"],
                     "--audio-quality", str(s["audio_quality"])]
        else:
            opts += ["-f", "bestaudio"]
    elif kind == "video no audio":
        height = "" if s["video_resolution"] == "best" else "[height<=%s]" % s["video_resolution"]
        opts += ["-f", "bv*%s" % height]
    else:
        if ffdir:
            if s["video_resolution"] == "best":
                height = ""
            else:
                height = "[height<=%s]" % s["video_resolution"]
            opts += ["-f", "bv*%s+ba/b" % height]
            if s["video_container"] not in ("best", ""):
                opts += ["--merge-output-format", s["video_container"]]
        else:
            opts += ["-f", "b"]

    if ffdir:
        opts += ["--ffmpeg-location", str(ffdir)]

    if s.get("playlist"):
        n = int(s.get("playlist_max_items") or 0)
        if n > 0:
            opts += ["--playlist-items", "1-%d" % n]
    else:
        opts += ["--no-playlist"]

    opts += ["--newline"]

    return opts + [url]


def build_sub_args(s, url, ffdir):
    """Subtitle-only pass: same output template, skip the video. Never
    counts as a failure for the download - it runs after the video file is
    on disk."""
    opts = output_args(s)
    opts += ["--skip-download",
             "--write-subs", "--write-auto-subs",
             "--sub-langs", sub_lang_patterns(s["subtitle_langs"]),
             "--no-playlist", "--newline"]
    opts += ["--convert-subs", "srt" if ffdir else "vtt"]
    if ffdir:
        opts += ["--ffmpeg-location", str(ffdir)]
    opts += js_runtime_args()
    return opts + [url]


# ---------------------------------------------------------------- download

FRAME_SECS = 0.1
DOTS = [".  ", ".. ", "...", ".. ", ".  "]


def flat_frames(frames, color):
    """Flat frames ("...", ">>>") as cell frames, one cell per character."""
    return [[(ch, color) for ch in f] for f in frames]


def flat_freeze(text, color):
    """A frozen flat marker as one wide cell, rendered exactly as before."""
    return [(text, color)]


def dot_stage(text):
    """The quiet amber dot stage for preflight, subtitle and ffmpeg waits.

    Cell colors are numeric palette indexes, 214 here - the ANSI strings
    (A_AMBER and friends) feed the wrapper c(), not render_cells."""
    return Stage(text, flat_frames(DOTS, 214), FRAME_SECS,
                 flat_freeze("...", 214))


# The eight waiting animations, ported from scripts-tools/spinner-demo.py.
# Each frame is six (char, color) cells so one frame can mix colors, and
# each animation carries its own tick and its own settled freeze frame.


def _cells(mapping):
    """Six-cell frame from {pos: (char, color)}; unset cells stay blank."""
    frame = [(" ", 0)] * 6
    for pos, cell in mapping.items():
        frame[pos] = cell
    return frame


_FIRE = [231, 223, 216, 214, 208, 202, 196, 88]
_GRAY = [255, 252, 249, 246, 243, 240, 236, 232]
_CYAN = [123, 87, 81, 51, 45, 39, 31, 30]
_MAGENTA = [213, 177, 141, 105, 99, 63, 62, 60]
_SPECTRUM = [196, 214, 226, 46, 33, 129]
_HEAT = [234, 238, 244, 250, 215, 208, 202, 196, 231]


def _comet():
    tail = ".", ":", "-", "=", "+", "*"
    path = list(range(6)) + list(range(4, 0, -1))
    frames = []
    for i, p in enumerate(path):
        step = 1 if i == 0 or p > path[i - 1] else -1
        m = {p: ("@", 231)}
        for k in range(1, 6):
            q = p - k * step
            if 0 <= q <= 5:
                m[q] = (tail[k - 1], _GRAY[min(k, len(_GRAY) - 1)])
        frames.append(_cells(m))
    return frames


def _pulse():
    frames = []
    for ring in ("*", "+", "."):
        bloom = []
        for t in range(3):
            m = {}
            for p in range(6):
                d = abs(p - 2.5)
                if d <= t + 0.5:
                    idx = int((d - 0.5) * 2)
                    if d > t:
                        m[p] = (ring, _FIRE[idx])
                    else:
                        m[p] = ("o", _FIRE[min(idx + 2, len(_FIRE) - 1)])
            bloom.append(_cells(m))
        frames.extend(bloom + bloom[-2::-1])
    return frames


def _orbit():
    frames = []
    for (cl, cr), hl, hr in ((123, 213), "o", "O"), ((46, 226), "e", "0"):
        for d in (1, 3, 5, 5, 3, 1):
            m = {}
            for p in range(6):
                dd = int(round(abs(p - 2.5) * 2))
                if dd < d:
                    m[p] = (":", 238)
                elif dd == d:
                    m[p] = (hl, cl) if p < 3 else (hr, cr)
            frames.append(_cells(m))
    return frames


def _scanner():
    path = list(range(6)) + list(range(4, 0, -1))
    frames = []
    for i, p in enumerate(path):
        m = {p: ("|", _SPECTRUM[i % 6])}
        for k in (1, 2, 3):
            for q in (p - k, p + k):
                if 0 <= q <= 5:
                    m[q] = (("." if k == 1 else ":" if k == 2 else "-"),
                            _GRAY[k + 1])
        frames.append(_cells(m))
    return frames


def _twinkle():
    glyph = "*", "+", "."
    frames = []
    for t in range(8):
        m = {}
        for p in range(6):
            seed = (p * 4 + t * 3) % 11
            if seed < 3:
                m[p] = (glyph[seed], _SPECTRUM[(p + t) % 6])
        frames.append(_cells(m))
    return frames


def _eq():
    ramp = " .:-=+*#@"
    frames = []
    for t in range(12):
        m = {}
        for p in range(6):
            lvl = int(round(4 + 3.6 * math.sin(t * 0.8 + p * 1.25)))
            lvl = max(0, min(len(ramp) - 1, lvl))
            m[p] = (ramp[lvl], _HEAT[lvl])
        frames.append(_cells(m))
    return frames


def _fill():
    def edge(t):
        m = {}
        for p in range(t):
            m[p] = ("=", _CYAN[t])
            m[5 - p] = ("=", _MAGENTA[t])
        return _cells(m)

    frames = [edge(t) for t in (0, 1, 2)]
    frames.append(_cells({p: ("=", _CYAN[3] if p < 3 else _MAGENTA[3])
                          for p in range(6)}))
    frames.append(_cells({p: ("#", 231) for p in range(6)}))
    frames.append(_cells({p: ("#", 231) for p in range(6)}))
    frames.extend(edge(t) for t in (2, 1, 0))
    return frames


def _bounce():
    path = 0, 0, 1, 2, 3, 4, 5, 5, 4, 3, 2, 1
    frames = []
    for hue in (_SPECTRUM[0], _SPECTRUM[3]):
        step = 1
        for i, p in enumerate(path):
            if i and p != path[i - 1]:
                step = 1 if p > path[i - 1] else -1
            head = "O" if i and p == path[i - 1] else "o"
            m = {p: (head, hue)}
            for k in range(1, 4):
                q = p - k * step
                if 0 <= q <= 5:
                    m[q] = (".", _GRAY[k + 1])
            frames.append(_cells(m))
    return frames


SPINNERS = [
    ("comet", _comet(), 0.08, _cells({2: ("@", 231)})),
    ("pulse", _pulse(), 0.08, _cells({2: ("o", _FIRE[2]), 3: ("o", _FIRE[2])})),
    ("orbit", _orbit(), 0.08, _cells({2: ("o", _CYAN[0])})),
    ("scanner", _scanner(), 0.08, _cells({2: ("|", _SPECTRUM[3])})),
    ("twinkle", _twinkle(), 0.12, _cells({2: ("*", 226)})),
    ("eq", _eq(), 0.09, _cells({p: ("=", _HEAT[5]) for p in range(6)})),
    ("fill", _fill(), 0.08,
     _cells({p: ("=", _CYAN[3] if p < 3 else _MAGENTA[3]) for p in range(6)})),
    ("bounce", _bounce(), 0.08, _cells({2: ("o", _SPECTRUM[3])})),
]


def pick_spinner():
    """A different waiting animation per download run, chosen at random."""
    return random.choice(SPINNERS)


def render_cells(frame):
    """ANSI render of one cell frame: a color code only when the color
    changes, reset at the end."""
    parts = []
    last = None
    for ch, col in frame:
        if col != last:
            parts.append("\033[38;5;%dm" % col)
            last = col
        parts.append(ch)
    parts.append(A_RESET)
    return "".join(parts)


def plain_cells(frame):
    """The frame without color, trimmed to column 0, for a piped run's
    frozen line."""
    return "".join(ch for ch, _col in frame).strip()


class Stage:
    """One animated stage line: cells spin, then the line freezes.

    Every rewrite clears the line first (cursor to column zero, erase to
    end of line), so a shorter progress value never leaves the tail of a
    longer one behind and the final verdict never shares its line with the
    last progress text. The animation runs the whole time: while live
    holds a progress text it keeps spinning and shows the text instead of
    the stage label. done() ends the line with a newline, so finished stages stay on screen
    under the input line until the next url, menu pick, or escape repaints
    the menu. Outside a tty the thread is skipped and only the final line
    prints."""

    def __init__(self, text, frames, tick, freeze):
        self.text = text
        self.frames = frames
        self.tick = tick
        self.freeze = freeze
        self.live = None
        self._stop = False
        self._th = None
        self._wlock = threading.Lock()

    def start(self):
        if not USE_COLOR:
            return
        self._th = threading.Thread(target=self._spin, daemon=True)
        self._th.start()

    def _spin(self):
        i = 0
        while not self._stop:
            with self._wlock:
                if self._stop:
                    break
                live = self.live
                frame = self.frames[i % len(self.frames)]
                text = live if live else self.text
                i += 1
                sys.stdout.write("\r\x1b[K%s %s" % (render_cells(frame), text))
                sys.stdout.flush()
            time.sleep(self.tick)

    def done(self, text=None):
        self._stop = True
        if self._th is not None:
            self._th.join(timeout=1.0)
        final = text if text is not None else self.text
        with self._wlock:
            if USE_COLOR:
                sys.stdout.write("\r\x1b[K%s %s\n"
                                 % (render_cells(self.freeze), final))
            else:
                sys.stdout.write("%s %s\n"
                                 % (plain_cells(self.freeze), final))
        sys.stdout.flush()


def ascii_safe(text):
    """Console-safe copy of yt-dlp output. Windows cp1252 stdout cannot
    encode titles like '... \uff5c ...' and would crash the print."""
    s = str(text)
    for bad, good in (("\uff5c", "|"), ("\u2013", "-"), ("\u2014", "-"),
                      ("\u2018", "'"), ("\u2019", "'"),
                      ("\u201c", "\""), ("\u201d", "\""),
                      ("\u2026", "..."), ("\u00a0", " ")):
        s = s.replace(bad, good)
    return s.encode("ascii", "replace").decode("ascii")


def sub_fail_reason(err):
    """Short reason for a failed subtitle pass, classified from yt-dlp's
    last error line. None when nothing recognizable - the caller then shows
    no second line."""
    low = (err or "").lower()
    if "429" in low or "too many requests" in low:
        return "the subtitle api answered 429 too many requests"
    if "timed out" in low or "timeout" in low:
        return "the subtitle request timed out"
    if "connection" in low or "network" in low or "unreachable" in low:
        return "the network was unreachable"
    return None


def sub_success_lines(codes):
    """Verdict lines for a subtitle pass that exited 0. Returns (verdict,
    miss_note); miss_note is empty when codes were read, else the audit F11
    parse-miss line - yt-dlp succeeded but our stdout scrape read nothing,
    which points at reworded upstream output rather than missing subs."""
    if codes:
        return ("subtitles - %s" % ", ".join(ascii_safe(x) for x in codes), "")
    return ("subtitles - none available",
            "note: yt-dlp reported success but no subtitle files were read "
            "- wording may have changed, please report")


def sub_fail_lines(err):
    """Verdict lines for a failed subtitle pass: the not-fetched verdict
    plus the classified reason, or the audit F11 note when the error line
    matched no known wording."""
    lines = ["subtitles - not fetched (video kept). "
             "unable to download subtitles"]
    reason = sub_fail_reason(err)
    if reason:
        lines.append("subtitle error - %s" % reason)
    else:
        lines.append("subtitle error - yt-dlp wording not recognized "
                     "- please report")
    return lines


def newest_since(folder, since, slack=2.0):
    """Newest file in folder with an mtime at or just after 'since'."""
    best = None
    try:
        for p in folder.glob("*"):
            if p.is_file():
                try:
                    m = p.stat().st_mtime
                except OSError:
                    continue
                if m >= since - slack and (best is None or m > best[0]):
                    best = (m, p)
    except OSError:
        return None
    return best[1] if best else None


def clean_partials(folder, since, slack=5.0):
    """Delete fresh *.part* and *.ytdl leftovers after a stopped download."""
    removed = []
    try:
        cands = list(folder.glob("*.part*")) + list(folder.glob("*.ytdl"))
    except OSError:
        return removed
    for p in cands:
        try:
            if p.stat().st_mtime >= since - slack and p.is_file():
                p.unlink()
                removed.append(p.name)
        except OSError:
            pass
    return removed


class KeyWatch:
    """Raw key watcher for the download run: only escape is acted on.

    Nothing else is consumed as input; the watcher exists so escape can stop
    a running download. No-op when stdin is not a tty."""

    def __init__(self):
        self.old = None
        self.fd = None
        if os.name == "nt":
            return
        if not sys.stdin.isatty():
            return
        import termios
        import tty
        self.fd = sys.stdin.fileno()
        self.old = termios.tcgetattr(self.fd)
        tty.setcbreak(self.fd)

    def escape_hit(self):
        if os.name == "nt":
            import msvcrt
            if not msvcrt.kbhit():
                return False
            ch = msvcrt.getwch()
            if ch in ("\x00", "\xe0"):  # eat arrow/function scan codes
                msvcrt.getwch()
                return False
            return ch == "\x1b"
        if self.fd is None:
            return False
        import select
        r, _, _ = select.select([self.fd], [], [], 0)
        if not r:
            return False
        return os.read(self.fd, 1) == b"\x1b"

    def restore(self):
        if self.old is not None:
            import termios
            termios.tcsetattr(self.fd, termios.TCSADRAIN, self.old)


def _kill_tree(proc):
    """Kill yt-dlp and any ffmpeg child it spawned."""
    if os.name == "nt":
        subprocess.call(["taskkill", "/F", "/T", "/PID", str(proc.pid)],
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL)
    else:
        import signal
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
        except OSError:
            pass
    proc.wait()


def run_download(binary, opts, stage=None):
    """Run yt-dlp with no input; escape stops it and cleans partials.

    stdout is piped so [download] progress lines can be shown on the stage
    line as "15.2% of 198.15MiB at 2.32MiB/s ETA 01:12", the same numbers as
    yt-dlp's own progress line. Subtitle write lines are collected for the
    subtitle stage sentence; stderr is captured last-lines-first for the
    failure sentence. Returns (rc, stopped, last_error, sub_codes); the error
    line is returned untruncated so sub_fail_reason can classify it and the
    callers truncate it for display."""
    popen_kw = {}
    if os.name != "nt":
        popen_kw["start_new_session"] = True
    started = time.time()
    proc = subprocess.Popen([str(binary)] + opts,
                            stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE,
                            **popen_kw)
    err_tail = []
    sub_codes = []

    def pump_out():
        for raw in proc.stdout:
            line = raw.decode("utf-8", "replace").rstrip() if isinstance(raw, bytes) else raw
            if line.startswith("[download]"):
                body = " ".join(line[len("[download]"):].split())
                # no Destination flash: it would replace the spinner frame
                if stage is not None and body and not body.startswith("Destination:"):
                    stage.live = color_progress(body) if USE_COLOR else body
            elif "Writing video subtitles to:" in line:
                name = line.split("Writing video subtitles to:", 1)[1].strip()
                code = name.rsplit(".", 2)[-2] if name.count(".") >= 2 else name
                sub_codes.append(code)

    def pump_err():
        for raw in proc.stderr:
            line = raw.decode("utf-8", "replace").rstrip() if isinstance(raw, bytes) else raw
            if line:
                err_tail.append(line)
                del err_tail[:-6]

    t_out = threading.Thread(target=pump_out, daemon=True)
    t_err = threading.Thread(target=pump_err, daemon=True)
    t_out.start()
    t_err.start()

    watch = KeyWatch()
    try:
        while True:
            rc = proc.poll()
            if rc is not None:
                break
            if watch.escape_hit():
                _kill_tree(proc)
                return -1, True, "", []
            time.sleep(FRAME_SECS)
    except KeyboardInterrupt:
        _kill_tree(proc)
        return -1, False, "cancelled", []
    finally:
        watch.restore()
    t_out.join(timeout=2.0)
    t_err.join(timeout=2.0)
    err = ""
    for line in reversed(err_tail):
        if "ERROR" in line or "error" in line.lower():
            err = line.strip()
            break
    if not err and err_tail:
        err = err_tail[-1].strip()
    return proc.returncode, False, ascii_safe(err), sub_codes


def fail_sentence(rc, err):
    """Frozen sentence for a failed run: the rc always, yt-dlp's error line
    when there is one, full length - the old err[:WIDTH] cut kept the error
    class but dropped the tail (a rename sharing violation lost the part
    naming the holder). Single line, ascii_safe already applied."""
    return "downloading - failed (rc=%s)%s" % (rc, ": " + err if err else "")


# --------------------------------------------------------------- ffmpeg fetch

def can_ask():
    """True only when a human is on the other end. A pipe, a redirect or CI
    must never be blocked by a download prompt, and cannot answer one."""
    try:
        return sys.stdin.isatty()
    except Exception:
        return False


def sha256_file(path, chunk=1 << 20):
    """Streaming sha256, so a 200MB download never sits in memory."""
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        while True:
            block = fh.read(chunk)
            if not block:
                break
            h.update(block)
    return h.hexdigest()


def parse_sha256(text):
    """The digest out of a single-asset checksum file. gyan.dev publishes a
    bare 64-char hash, a github checksums.sha256 publishes "<hash>  <file>",
    so take the first field that is a digest rather than assuming a shape."""
    for field in text.split():
        cand = field.lower().strip("*")
        if len(cand) == 64 and re.fullmatch(r"[0-9a-f]{64}", cand):
            return cand
    return None


def pick_checksum(text, filename):
    """The digest for one asset out of a multi-asset checksums file, where the
    digest is the line that names that file."""
    for line in text.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[1].strip("*") == filename:
            cand = parts[0].lower()
            if len(cand) == 64 and re.fullmatch(r"[0-9a-f]{64}", cand):
                return cand
    return None


def http_text(url, timeout=60):
    req = urllib.request.Request(url, headers={"User-Agent": "yt-dlp-cli"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read().decode("utf-8", "replace")


def http_file(url, dest, stage=None, timeout=DL_TIMEOUT):
    """Stream url to dest through a .part name, so a failure never leaves a
    half-written binary that a later run would mistake for a good one. Returns
    (bytes, final url, content length)."""
    tmp = dest.with_name(dest.name + ".part")
    got, total, final = 0, 0, url
    req = urllib.request.Request(url, headers={"User-Agent": "yt-dlp-cli"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        final = r.geturl()
        hdr = r.headers.get("Content-Length") or ""
        total = int(hdr) if hdr.isdigit() else 0
        with open(tmp, "wb") as fh:
            while True:
                block = r.read(1 << 16)
                if not block:
                    break
                fh.write(block)
                got += len(block)
                if stage is not None and total:
                    stage.live = "downloading ffmpeg - %.1f / %.1f MB" % (
                        got / 1048576.0, total / 1048576.0)
    os.replace(tmp, dest)
    return got, final, total


def expected_sha(url, sha_url, asset_name):
    """The published digest for the asset. Raises nothing: returns None with a
    reason printed, because a download with nothing to check it against is not
    a download this CLI should perform."""
    try:
        text = http_text(sha_url)
    except Exception as exc:
        print("  could not read the published checksum: %s" % exc)
        return None
    digest = pick_checksum(text, asset_name) or parse_sha256(text)
    if digest is None:
        print("  the published checksum did not name this build")
    return digest


def extract_member(blob, how, binname, outdir):
    """Lift the single needed binary out of a downloaded archive into outdir.
    Every provider nests one versioned folder with bin/<binname> inside, so the
    nested part is skipped and only the binary is written. Returns the path or
    None."""
    out = outdir / binname
    try:
        if how == "zip":
            with zipfile.ZipFile(blob) as zf:
                named = [n for n in zf.namelist()
                         if n.rsplit("/", 1)[-1] == binname
                         and not n.endswith("/")]
                if not named:
                    return None
                with zf.open(named[0]) as src, open(out, "wb") as dst:
                    shutil.copyfileobj(src, dst)
        elif how == "tar.xz":
            with tarfile.open(blob, "r:xz") as tf:
                hit = None
                for m in tf.getmembers():
                    if m.isfile() and m.name.rsplit("/", 1)[-1] == binname:
                        hit = m
                        break
                if hit is None:
                    return None
                with tf.extractfile(hit) as src, open(out, "wb") as dst:
                    shutil.copyfileobj(src, dst)
        else:
            return None
    except (zipfile.BadZipFile, tarfile.TarError, OSError, EOFError) as exc:
        print("  the downloaded archive was unreadable: %s" % exc)
        return None
    return out


def install_ffmpeg(stage=None):
    """Fetch, verify, test and install ffmpeg into bin/ for this platform.

    Verification order is deliberate: the published digest is read first and the
    download is discarded on any mismatch, and the extracted binary must answer
    --version on this machine before it is moved over whatever is already in
    bin/. So a truncated or wrong-platform build can never replace a working
    ffmpeg. Returns the binary path, or None with the reason printed."""
    plat = platform_name()
    src = FFMPEG_SOURCES.get(plat)
    if src is None:
        print("  no published ffmpeg download for %s" % plat)
        print("  install it with your package manager - it is then found on PATH")
        return None
    url, sha_url, binname, how = src
    asset = url.rsplit("/", 1)[-1]
    digest = expected_sha(url, sha_url, asset)
    if digest is None:
        return None
    try:
        BIN_DIR.mkdir(parents=True, exist_ok=True)
    except OSError as exc:
        print("  bin/ is not writable: %s" % exc)
        return None

    tmpdir = Path(tempfile.mkdtemp(prefix=".ff-", dir=str(BIN_DIR)))
    try:
        blob = tmpdir / asset
        try:
            http_file(url, blob, stage)
        except Exception as exc:
            print("  download failed: %s" % ascii_safe(exc))
            return None
        if stage is not None:
            stage.live = "verifying the checksum"
        if sha256_file(blob) != digest:
            print("  checksum mismatch - nothing was changed")
            return None
        if stage is not None:
            stage.live = "unpacking"
        fresh = extract_member(blob, how, binname, tmpdir)
        if fresh is None:
            print("  no %s inside the downloaded archive" % binname)
            return None
        if stage is not None:
            stage.live = "testing the binary"
        # one exec can fail transiently right after a large download burst, so
        # probe three times before declaring the build unusable. a genuinely
        # incompatible binary (wrong arch, missing glibc) fails all three.
        ver, rc = "", -1
        for attempt in range(3):
            ver, rc = probe_version(fresh)
            if rc == 0 and ver:
                break
            if attempt < 2:
                time.sleep(1.0)
        if rc != 0 or not ver:
            print("  the downloaded ffmpeg did not run on this system"
                  " (rc=%s)" % rc)
            return None
        target = BIN_DIR / binname
        os.replace(fresh, target)
        if os.name != "nt":
            os.chmod(target, 0o755)
        return target
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)


def offer_ffmpeg(s, kind):
    """A lean install that has no ffmpeg: offer the published build before the
    run silently degrades to single files. Returns the resolved ffmpeg dir on
    success, else None. Silent, and declining, when nobody can answer."""
    ffdir = find_ffmpeg_dir(s)
    if ffdir:
        return ffdir
    if not can_ask():
        return None
    plat = platform_name()
    src = FFMPEG_SOURCES.get(plat)
    if kind == "audio only":
        why = "without it the audio is saved as-is, no mp3/opus/wav conversion"
    else:
        why = "without it you get one single best file - no separate audio" \
              " track, no merge, no mp3, no srt subtitles"
    print("")
    print(c(A_AMBER, "  ffmpeg is not installed"))
    print("  %s" % why)
    if src is None:
        print("  no published download for %s - install it with your package"
              " manager instead" % plat)
        return None
    url, _sha, _bin, how = src
    print("  download the official %s build? (%s)" % (plat, "zip" if how == "zip"
                                                      else "tar.xz"))
    print("  %s" % url)
    raw = read_esc("  y/n (enter = no): ")
    if raw is None or raw.strip().lower() not in ("y", "yes"):
        print("  continuing without ffmpeg")
        return None
    st = dot_stage("downloading ffmpeg")
    st.start()
    try:
        got = install_ffmpeg(st)
    finally:
        st.done("ffmpeg - %s" % ("installed in bin/" if got else "not installed"))
    if got is None:
        return None
    ffdir = find_ffmpeg_dir(s)
    if ffdir is None:
        print("  installed, but it is still not found - check ffmpeg_path")
        return None
    print("  ffmpeg ready: %s" % ffdir)
    return ffdir


def do_download(s, url):
    if not (url or "").lower().startswith(("http://", "https://")):
        print("not a URL: %r" % url)
        return 2
    binary = find_yt_dlp(s)
    if binary is None:
        print("%s yt-dlp not found - put it in bin/ or on PATH,"
              " or set yt_dlp_path in config/settings.json"
              % c(A_AMBER, "..."))
        return 2

    # sub-second preflight checks share one grouped stage line
    st = dot_stage("checking yt-dlp and ffmpeg")
    st.start()
    ver, vrc = probe_version(binary)
    ffdir = find_ffmpeg_dir(s)
    if vrc != 0 or not ver:
        st.done("yt-dlp check failed (rc=%s): %s" % (vrc, ver))
        return 2
    kind = s.get("type", "video")
    if ffdir:
        ffs = "ffmpeg found"
    elif kind == "audio only":
        ffs = "no ffmpeg - saving original audio"
    else:
        ffs = "no ffmpeg - single best file"
    st.done("ready - yt-dlp v%s, %s" % (ver, ffs))

    if ffdir is None:
        ffdir = offer_ffmpeg(s, kind)

    opts = build_args(s, url, ffdir)
    started = time.time()
    _sp_name, sp_frames, sp_tick, sp_freeze = pick_spinner()
    st = Stage("downloading (escape to stop)", sp_frames, sp_tick, sp_freeze)
    st.start()
    rc, stopped, err, _codes = run_download(binary, opts, st)
    if stopped:
        removed = clean_partials(resolve_outdir(s), started)
        tail = ""
        if removed:
            tail = ", cleaned %d partial file%s" % (len(removed),
                                                    "" if len(removed) == 1
                                                    else "s")
        st.done("downloading - stopped by escape%s" % tail)
        return 130
    if rc != 0:
        st.done(fail_sentence(rc, err))
        return rc

    st.done("downloading - finished")
    newest = newest_since(resolve_outdir(s), started)
    if newest is None:
        print("saved to %s" % resolve_outdir(s))
    else:
        print("%s done - %s (%.1fMB)" % (c(A_AMBER, "..."),
                                         ascii_safe(newest.name),
                                         newest.stat().st_size / 1048576.0))

    if s.get("subtitles"):
        st2 = dot_stage("subtitles (%s)" % s["subtitle_langs"])
        st2.start()
        rc2, _stopped2, err2, codes = run_download(binary,
                                                    build_sub_args(s, url, ffdir),
                                                    st2)
        if rc2 == 0:
            verdict, miss = sub_success_lines(codes)
            st2.done(verdict)
            if miss:
                st2.done(miss)
        else:
            lines = sub_fail_lines(err2)
            st2.done(lines[0])
            st2.done(lines[1])
    return 0


# ---------------------------------------------------------------- menu parts

def sel_mark(sel):
    """Selection box: amber [x] when chosen, plain [ ] otherwise."""
    return c(A_AMBER, "[x]") if sel else "[ ]"


def hold():
    """Wait for enter without printing a prompt.

    The menu clears the screen and re-renders the instant a submenu returns,
    so a line printed after that submenu's last read is wiped in the same
    instant. A caller with something to say prints it, then holds. Unlabelled
    on purpose, so the message stays the last thing visible."""
    read_esc("")


def pick_list(title, options, current):
    print("  %s" % title)
    for i, o in enumerate(options, 1):
        print("    %s %2d  %s" % (sel_mark(str(o) == str(current)), i, o))
    raw = read_esc("  number (enter = keep %s): " % current)
    if raw is None:
        raise GoBack()
    raw = raw.strip()
    if not raw:
        return current
    try:
        idx = int(raw)
    except ValueError:
        print("  invalid number")
        hold()
        return current
    if 1 <= idx <= len(options):
        return options[idx - 1]
    print("  out of range")
    hold()
    return current


def ask_bool(label, current):
    raw = read_esc("  %s (now %s) y/n (enter = keep): " % (label, onoff(current)))
    if raw is None:
        raise GoBack()
    raw = raw.strip().lower()
    if raw in ("y", "yes", "on", "1"):
        return True
    if raw in ("n", "no", "off", "0"):
        return False
    return current


def batch_row(n, text):
    """One numbered row of the multi-url list: the url, or an error row
    when the entry holds no http(s) link. A wrong paste is kept in the list
    and marked, never dropped and never a showstopper for the rest."""
    url = clean_url(text)
    if url:
        return c(A_GREY, "  %d  " % n) + url
    return c(A_GREY, "  %d  " % n) + c(A_AMBER, "error in url")


def paint_batch(s, entries):
    """Menu on top, the collected list under it, input line last."""
    if USE_COLOR:
        sys.stdout.write(CLEAR)
        sys.stdout.flush()
    render(s)
    hint(s)
    print(c(A_AMBER, "  multiple urls - one per line,"
                     " empty line starts the downloads"))
    print(c(A_DARKGREY, "  escape returns to the menu"))
    for n, text in enumerate(entries, 1):
        print(batch_row(n, text))


def run_batch(s, entries, download=None):
    """Download the collected entries in order, one at a time - no
    concurrency, so youtube sees the same single-connection pattern as a
    normal download. The menu is repainted above every run.

    An entry with no http(s) link is marked "error in url" and skipped.
    Escape during a run stops that run and the whole batch.

    Returns True when at least one download started, so the caller knows
    whether the screen still needs a repaint."""
    download = download or do_download
    total = len(entries)
    started = False
    for i, text in enumerate(entries, 1):
        if USE_COLOR:
            sys.stdout.write(CLEAR)
            sys.stdout.flush()
        render(s)
        hint(s)
        url = clean_url(text)
        print(c(A_GREY, "  url %d of %d" % (i, total)) + "  "
              + (url if url else c(A_AMBER, "error in url")))
        if url is None:
            print("  %s not a url - skipped" % c(A_AMBER, "..."))
            continue
        if download(s, url) == 130:
            break
        started = True
    return started


def multi_url(s):
    """Menu item 9: collect urls one per line, then download them in order.

    Paste a url, enter, paste the next; an empty line is the enter that
    confirms the end of the list and starts the downloads, escape returns to
    the menu at any point. Returns True when the main menu must repaint,
    False when downloads ran - the screen then already holds the menu with
    the last run's result lines underneath it."""
    entries = []
    while True:
        paint_batch(s, entries)
        try:
            raw = read_esc("  url %d: " % (len(entries) + 1))
        except (EOFError, KeyboardInterrupt):
            return True
        if raw is None:
            return True
        text = (raw or "").strip()
        if not text:
            break
        entries.append(text)
    if not entries:
        return True
    return not run_batch(s, entries)


def dwidth(text):
    """Display width in terminal columns (audit F10): east-asian wide and
    fullwidth code points take 2 columns (Unicode EastAsianWidth via the
    stdlib unicodedata), everything else 1. len() counts code points, not
    columns, so a CJK or emoji value overflowed the menu box."""
    return sum(2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1
               for ch in text)


def cut_to_width(text, maxw):
    """Longest prefix of text whose display width is <= maxw."""
    out = []
    w = 0
    for ch in text:
        cw = 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1
        if w + cw > maxw:
            break
        out.append(ch)
        w += cw
    return "".join(out)


def tail_to_width(text, maxw):
    """Longest suffix of text whose display width is <= maxw."""
    out = []
    w = 0
    for ch in reversed(text):
        cw = 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1
        if w + cw > maxw:
            break
        out.append(ch)
        w += cw
    return "".join(reversed(out))


def menu_line(idx, label, value="", grey=True):
    plain = " %2d  %s" % (idx, label)
    left = c(A_GREY, plain) if grey else plain
    maxv = WIDTH - dwidth(plain) - 3
    if maxv < 1:
        maxv = 1
    if dwidth(value) > maxv:
        value = ("..." + tail_to_width(value, maxv - 3)) if maxv > 3 \
            else cut_to_width(value, maxv)
    pad = WIDTH - dwidth(plain) - dwidth(value)
    if pad < 1:
        pad = 1
    m = c(A_DARKGREY, "|")
    return m + left + (" " * pad) + value + m


def short_path(p, maxv):
    """Path for menu item 5: always '...\\<folder name>'."""
    sep = "\\" if os.name == "nt" else "/"
    parts = [x for x in p.replace("/", sep).split(sep) if x]
    name = parts[-1] if parts else "downloads"
    out = "..." + sep + name
    if dwidth(out) <= maxv:
        return out
    return cut_to_width(out, maxv)


def sub_lang_lines(width=WIDTH):
    """The subtitle code reference for menu item 7: every known code, space
    separated, alphabetical, filled to the box width. Pure function, so the
    selftest can pin the layout without a tty."""
    lines = []
    row = ""
    for code in sorted(KNOWN_LANG_CODES):
        if not row:
            row = code
        elif len(row) + 1 + len(code) + 2 <= width:
            row += " " + code
        else:
            lines.append("  " + row)
            row = code
    if row:
        lines.append("  " + row)
    return lines


def hint(s):
    """Amber instructions plus grey detail lines, above the input field."""
    div = "-" * (WIDTH + 2)
    l1 = "paste url, press enter and download will start."
    l2 = "enter number to change default settings"
    l3 = "change download location by choosing menu item: 5"
    l3b = "download several urls at once by choosing menu item: 9"
    l4 = "DL location: %s" % resolve_outdir(s)
    print(c(A_AMBER, l1))
    print(c(A_AMBER, l2))
    print(c(A_DARKGREY, l3))
    print(c(A_DARKGREY, l3b))
    print(c(A_DARKGREY, l4))
    print(c(A_LBLUE, div))


def header():
    title = "yt-dlp cli  v%s" % VERSION
    text = " " + title + " "
    pad = WIDTH - len(text)
    left = pad // 2
    right = pad - left
    div = "-" * (WIDTH + 2)
    # dotted lines above/below and the side markers are light blue,
    # the centered header text itself is amber
    print(c(A_LBLUE, div))
    if USE_COLOR:
        line = (A_LBLUE + "|" + A_RESET + " " * left
                + A_AMBER + text + A_RESET
                + " " * right + A_LBLUE + "|" + A_RESET)
    else:
        line = "|" + " " * left + text + " " * right + "|"
    print(line)
    print(c(A_LBLUE, div))


def menu_items(s):
    """The menu rows as (number, label, value, grey), numbered 1 to 13."""
    kind = s.get("type", "video")
    res = s["video_resolution"]
    if kind == "audio only":
        shown = "audio only (%s)" % s["audio_format"]
        container = "-"
        media = "-"
    elif kind == "video no audio":
        shown = "video no audio"
        container = "-"
        media = res if res == "best" else res + "p"
    else:
        shown = "video"
        container = s["video_container"]
        media = res if res == "best" else res + "p"
    if s.get("playlist"):
        n = int(s.get("playlist_max_items") or 0)
        pl = "on (all)" if n <= 0 else "on (max %d)" % n
    else:
        pl = "off"
    items = [
        (1, "video/audio", shown, True),
        (2, "format container", container, True),
        (3, "video resolution", media, True),
        (4, "audio format", s["audio_format"], True),
        (5, "output folder",
         short_path(str(resolve_outdir(s)),
                    WIDTH - len("  5  output folder") - 3), True),
        (6, "filename template", s["filename_template"], True),
        (7, "subtitles", "%s (%s)" % (onoff(s.get("subtitles")), s["subtitle_langs"]), True),
        (8, "playlist on/off", pl, True),
        (9, "add multiple youtube urls", "", True),
        (10, "reset defaults", "config/settings.json", True),
        (11, "update yt-dlp + cli", "", True),
        (12, "auto update", onoff(s.get("auto_update")), True),
        (13, "quit", "", False),
    ]
    return items


def render(s):
    items = menu_items(s)
    header()
    m = c(A_DARKGREY, "|")
    print(m + " " * WIDTH + m)
    for idx, label, value, grey in items:
        print(menu_line(idx, label, value, grey))
    desc = "press escape to exit submenu"
    pad = WIDTH - len(desc)
    left = pad // 2
    right = pad - left
    print(m + " " * left + c(A_LBLUE, desc) + " " * right + m)
    print(c(A_DARKGREY, "-" * (WIDTH + 2)))


def menu_summary(s):
    if s.get("type") == "audio only":
        return "audio-only %s" % s["audio_format"]
    return "%s %sp" % (s["video_container"], s["video_resolution"])


def main_menu(s):
    enable_vt_output()
    dirty = True
    results = False
    started = False
    while True:
        if dirty:
            if USE_COLOR:
                sys.stdout.write(CLEAR)
                sys.stdout.flush()
            render(s)
            hint(s)
            dirty = False
            results = False
        if not started:
            # after the first paint, so the notice is not wiped by the clear
            started = True
            auto_update_check(s)
            save_settings(s)
        try:
            choice = read_esc("> ")
        except (EOFError, KeyboardInterrupt):
            print()
            return 0
        if choice is None:
            # escape: wipe the results if any are on screen, else a no-op
            if results:
                dirty = True
            continue
        choice = choice.strip()
        url = clean_url(choice)
        if url:
            # a new url starts from a fresh menu on top, results below input
            if USE_COLOR:
                sys.stdout.write(CLEAR)
                sys.stdout.flush()
            render(s)
            hint(s)
            do_download(s, url)
            results = True
            save_settings(s)
            continue
        try:
            repaint = act(choice, s)
        except GoBack:
            repaint = True
        # item 9 answers False when downloads ran: the screen then already
        # holds the menu with the last result lines under it, and those stay
        # until the next url, menu pick or escape
        dirty = bool(repaint)
        results = not dirty
        save_settings(s)

def folder_dialog(initial):
    """Native folder picker. Returns a path or None (cancel / not available).

    Silently returns None when not attached to a console, so piped/test runs
    never trigger a GUI."""
    if not (sys.stdin.isatty() and sys.stdout.isatty()):
        return None
    if os.name == "nt":
        return _win_folder_dialog(initial)
    return _posix_folder_dialog(initial)


def _win_folder_dialog(initial):
    import ctypes
    from ctypes import wintypes

    class BROWSEINFO(ctypes.Structure):
        _fields_ = [("hwndOwner", wintypes.HWND),
                    ("pidlRoot", ctypes.c_void_p),
                    ("pszDisplayName", ctypes.c_wchar_p),
                    ("lpszTitle", ctypes.c_wchar_p),
                    ("ulFlags", ctypes.c_uint),
                    ("lpfn", ctypes.c_void_p),
                    ("lParam", ctypes.c_long),
                    ("iImage", ctypes.c_int)]

    shell32 = ctypes.windll.shell32
    shell32.SHBrowseForFolderW.restype = ctypes.c_void_p
    shell32.SHGetPathFromIDListW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p]
    display = ctypes.create_unicode_buffer(wintypes.MAX_PATH)
    info = BROWSEINFO()
    info.pszDisplayName = ctypes.cast(display, ctypes.c_wchar_p)
    info.lpszTitle = "choose download folder"
    info.ulFlags = 0x00000001 | 0x00000010 | 0x00000040  # fsonly | editbox | newstyle
    pidl = shell32.SHBrowseForFolderW(ctypes.byref(info))
    if not pidl:
        return None
    path = ctypes.create_unicode_buffer(wintypes.MAX_PATH)
    shell32.SHGetPathFromIDListW(pidl, path)
    ctypes.windll.ole32.CoTaskMemFree(ctypes.c_void_p(pidl))
    return path.value or None


def _posix_folder_dialog(initial):
    try:
        import tkinter
        from tkinter import filedialog
        root = tkinter.Tk()
        root.withdraw()
        try:
            picked = filedialog.askdirectory(title="choose download folder",
                                             initialdir=initial or "/",
                                             mustexist=True)
        finally:
            root.destroy()
        return picked or None
    except Exception:
        pass
    for cmd in (["zenity", "--file-selection", "--directory"],
                ["kdialog", "--getexistingdirectory"]):
        if not shutil.which(cmd[0]):
            continue
        try:
            r = subprocess.run(cmd + ([str(initial)] if initial else []),
                               capture_output=True, text=True, timeout=120)
        except Exception:
            continue
        if r.returncode == 0 and r.stdout.strip():
            return r.stdout.strip()
    return None


def update_yt_dlp(s):
    """In-place self-update of the resolved yt-dlp binary (`<binary> -U`).

    Returns True when the binary answered -U, False when it could not be
    found or failed its version check."""
    binary = find_yt_dlp(s)
    if binary is None:
        print("yt-dlp binary not found.")
        print("put yt-dlp in bin/ or on PATH, or set yt_dlp_path"
              " in config/settings.json")
        return False
    old, rc = probe_version(binary)
    if rc != 0 or not old:
        print("yt-dlp binary check failed (rc=%s): %s" % (rc, old))
        return False
    print("binary      : %s" % binary)
    print("version now : %s" % old)
    try:
        rc = subprocess.call([str(binary), "-U"])
    except KeyboardInterrupt:
        print("\ncancelled by user")
        return False
    new, _ = probe_version(binary)
    print("version after: %s" % new)
    return rc == 0


def version_tuple(v):
    """A version string as a comparable tuple. Non-numeric pieces count as 0
    rather than raising, because the published string is not under our control."""
    parts = [int(p) if p.isdigit() else 0
             for p in re.split(r"[.\-+]", str(v or "")) if p != ""]
    return tuple(parts) if parts else (0,)


def running_from_source():
    """True when this file is a plain .py and so may replace itself. A zipapp
    or a frozen exe must be replaced by reinstalling the bundle."""
    return CLI_SELF_PATH.suffix == ".py" and CLI_SELF_PATH.is_file()


def fetch_cli(url=CLI_UPDATE_URL, timeout=60):
    """(published VERSION, raw bytes) from one request. Reading the version and
    the replacement from the same bytes means no second request can disagree
    with the first."""
    req = urllib.request.Request(url, headers={"User-Agent": "yt-dlp-cli"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        raw = r.read().decode("utf-8", "replace")
    m = re.search(r'^VERSION\s*=\s*"([^"]*)"', raw, re.M)
    return (m.group(1) if m else None), raw


def install_cli(raw, target=None):
    """Replace this file with the published bytes. The new bytes are parsed
    before they are written and land beside the target first, so a truncated or
    syntax-broken download can never leave the CLI unbootable. Returns
    (ok, message)."""
    target = target or CLI_SELF_PATH
    if not running_from_source():
        return False, ("this is not a plain .py file - update by reinstalling"
                       " the bundle")
    try:
        compile(raw, str(target), "exec")
    except SyntaxError as exc:
        return False, "the published version did not parse (%s)" % exc
    tmp = target.with_name(target.name + ".new")
    try:
        with open(tmp, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(raw)
        os.replace(tmp, target)
    except OSError as exc:
        try:
            tmp.unlink()
        except OSError:
            pass
        return False, "could not write the new file: %s" % exc
    return True, str(target)


def auto_update_check(s, force=False):
    """Startup notice for a newer published version, or the forced form menu
    item 11 runs. It asks before it touches anything and, unless forced,
    throttles itself to one network check a day and stays silent when the
    network is gone. Returns True only when the file was replaced."""
    if not force:
        if not s.get("auto_update"):
            return False
        if not can_ask():
            return False
        try:
            last = float(s.get("auto_update_last_check") or 0)
        except (TypeError, ValueError):
            last = 0.0
        if time.time() - last < 86400:
            return False
    try:
        newver, raw = fetch_cli(timeout=60 if force else CHECK_TIMEOUT)
    except Exception as exc:
        if force:
            print("  could not reach the update source: %s" % ascii_safe(exc))
        return False
    if not force:
        s["auto_update_last_check"] = int(time.time())
    if newver is None:
        if force:
            print("  no version found in the published file")
        return False
    if version_tuple(newver) <= version_tuple(VERSION):
        if force:
            print("  published v%s, this is v%s - already current"
                  % (newver, VERSION))
        return False
    print("  a newer version is published: v%s (this is v%s)"
          % (newver, VERSION))
    print("  %s" % CLI_UPDATE_URL)
    raw = read_esc("  update now? y/n (enter = no): ")
    if raw is None or raw.strip().lower() not in ("y", "yes"):
        print("  not updated")
        return False
    if not force:
        s["auto_update_last_check"] = int(time.time())
    ok, msg = install_cli(raw)
    print("  %s" % msg)
    if ok:
        print("  updated - the new version runs from the next start")
    return ok


def update_all(s):
    """Menu item 11: both updatable halves, the yt-dlp runtime and this CLI.
    Returns True when either one moved."""
    print("  yt-dlp")
    moved_yt = update_yt_dlp(s)
    print("  cli")
    moved_cli = auto_update_check(s, force=True)
    return bool(moved_yt or moved_cli)


def act(choice, s):
    """One menu pick. Returns True when the main menu must repaint.

    Only item 9 returns False, and only when downloads ran: the screen then
    already shows the menu with the last result lines underneath it."""
    if choice == "1":
        s["type"] = pick_list("change type: video / video no audio / audio only",
                              TYPES, s.get("type", "video"))
    elif choice == "2":
        if s.get("type", "video") == "audio only":
            print("  audio only mode has no container")
            return
        s["video_container"] = pick_list("container", CONTAINERS, s["video_container"])
    elif choice == "3":
        if s.get("type", "video") == "audio only":
            print("  audio only mode has no resolution")
            return
        s["video_resolution"] = pick_list("max height", RESOLUTIONS, s["video_resolution"])
    elif choice == "4":
        s["audio_format"] = pick_list("audio format (needs ffmpeg to convert)", AUDIO_FORMATS, s["audio_format"])
    elif choice == "5":
        picked = folder_dialog(resolve_outdir(s))
        if picked:
            s["output_dir"] = picked
            return
        raw = read_esc("  folder (enter = keep %s): " % resolve_outdir(s))
        if raw is None:
            raise GoBack()
        raw = raw.strip().strip('"')
        if raw:
            s["output_dir"] = raw
    elif choice == "6":
        print("  filename template - standard templates:")
        for i, t in enumerate(TEMPLATE_PRESETS, 1):
            print("    %s %2d  %s"
                  % (sel_mark(t == s["filename_template"]), i, t))
        raw = read_esc("  number 1-%d, or type a template (enter = keep): "
                       % len(TEMPLATE_PRESETS))
        if raw is None:
            raise GoBack()
        raw = raw.strip()
        if not raw:
            pass
        elif raw.isdigit() and 1 <= int(raw) <= len(TEMPLATE_PRESETS):
            s["filename_template"] = TEMPLATE_PRESETS[int(raw) - 1]
        else:
            s["filename_template"] = raw
    elif choice == "7":
        s["subtitles"] = ask_bool("subtitles", s.get("subtitles"))
        if s.get("subtitles"):
            plain = sub_lang_lines()
            block = [c(A_DARKGREY, line) for line in plain]
            under = block_under_input(plain)
            if not under:
                # no room under a live cursor: the block goes above the input,
                # where it is on screen while the value is being typed
                for line in block:
                    print(line)
            print("  enter languages separated by commas (enter = keep):")
            raw = read_esc("", below=block if under else None)
            if raw is None:
                raise GoBack()
            raw = raw.strip()
            if raw:
                kept, bad, unknown = parse_lang_input(raw)
                if kept:
                    s["subtitle_langs"] = ",".join(kept)
                said = []
                if bad:
                    said.append("not a language code, dropped: "
                                + ", ".join(bad))
                if unknown:
                    said.append("not in the language list, kept: "
                                + ", ".join(unknown))
                if bad and not kept:
                    said.append("nothing valid left, kept "
                                + str(s["subtitle_langs"]))
                if under and said:
                    # the input row becomes the verdict and the block is
                    # repainted under it, so nothing prints below the block.
                    # after Enter the caret sits on the first block row, so
                    # step back onto the input row before rewriting it.
                    sys.stdout.write("\033[1A\r\033[2K")
                    for text in said:
                        print(c(A_AMBER, "  " + text))
                    for line in block:
                        print(line)
                    sys.stdout.write("\033[%dA" % (len(block) + 1))
                    sys.stdout.flush()
                    hold()
                else:
                    for text in said:
                        print(c(A_AMBER, "  " + text))
                    if not under:
                        hold()
    elif choice == "8":
        s["playlist"] = ask_bool("playlist mode", s.get("playlist"))
        if s.get("playlist"):
            raw = read_esc("  max items (0 = all, enter = keep %s): "
                           % s.get("playlist_max_items"))
            if raw is None:
                raise GoBack()
            raw = raw.strip()
            if raw:
                try:
                    s["playlist_max_items"] = int(raw)
                except ValueError:
                    print("  not a number - kept %s"
                          % s.get("playlist_max_items"))
                    hold()
    elif choice == "9":
        return multi_url(s)
    elif choice == "10":
        for k, v in DEFAULTS.items():
            s[k] = json.loads(json.dumps(v)) if isinstance(v, (dict, list)) else v
        print("  defaults restored")
        hold()
    elif choice == "11":
        update_all(s)
        read_esc("  press enter to go back: ")
    elif choice == "12":
        s["auto_update"] = not s.get("auto_update")
        # no save here (audit F8): main_menu saves after act() returns, so
        # every action has exactly one save site; quit keeps its own.
        print("  auto update %s - checked once a day at start"
              % onoff(s.get("auto_update")))
        hold()
    elif choice in ("13", "q", "quit", "exit"):
        save_settings(s)
        print("bye. defaults saved to %s" % CONFIG_PATH)
        raise SystemExit(0)
    elif choice:
        print("  pick 1-13 or paste a url")
    return True


# ---------------------------------------------------------------- cli/self-test

def show_check(s):
    binary = find_yt_dlp(s)
    ffdir = find_ffmpeg_dir(s)
    print("platform      : %s" % platform_name())
    print("project root  : %s" % ROOT)
    print("yt-dlp binary : %s" % (binary or "MISSING"))
    if binary:
        ver, rc = probe_version(binary)
        print("yt-dlp version: %s (rc=%s)" % (ver, rc))
    print("ffmpeg dir    : %s" % (ffdir or "MISSING - single files / no convert"))
    print("output folder : %s" % resolve_outdir(s))
    print("settings file : %s (exists=%s)" % (CONFIG_PATH, CONFIG_PATH.exists()))


def selftest():
    fails = []
    count = [0]

    def check(name, cond, got=""):
        count[0] += 1
        ok = "PASS" if cond else "FAIL"
        print("%-4s %s%s" % (ok, name, "" if cond else "  got: %r" % (got,)))
        if not cond:
            fails.append(name)

    def a_index(argv, flag):
        return argv[argv.index(flag) + 1] if flag in argv else None

    import io
    cap = io.StringIO()
    real_out = sys.stdout
    sys.stdout = cap
    try:
        HISTORY.clear()
        HISTORY.extend(["https://a.example", "https://b.example"])
        ed = LineEdit("> ")
        for ch in "xy":
            ed.add(ch)
        ed.recall(True)
        v1 = ed.text()
        ed.recall(True)
        v2 = ed.text()
        ed.recall(False)
        v3 = ed.text()
        ed.recall(False)
        v4 = ed.text()
        ed2 = LineEdit("> ")
        ed2.add("q")
        ed2.remember()
        v5 = HISTORY[-1]
        ed3 = LineEdit("> ")
        ed3.remember()
        v6 = HISTORY[-1]
    finally:
        sys.stdout = real_out
    check("history newest first", v1 == "https://b.example"
          and v2 == "https://a.example", (v1, v2))
    check("history returns to draft", v3 == "https://b.example"
          and v4 == "xy", (v3, v4))
    check("history remembers submitted", v5 == "q", v5)
    check("history skips empty lines", v6 == "q", v6)

    r429_src = ("ERROR: Unable to download video subtitles "
                "for 'en': HTTP Error 429: Too Many Requests")
    r429 = sub_fail_reason(r429_src)
    check("sub reason 429", r429 == "the subtitle api answered 429 too many requests", r429)
    check("sub reason unknown -> none", sub_fail_reason("SOMETHING ODD") is None,
          sub_fail_reason("SOMETHING ODD"))

    sv, sm = sub_success_lines(["en", "en-GB"])
    check("sub success verdict joins codes",
          sv == "subtitles - en, en-GB", sv)
    check("sub success with codes has no miss note", sm == "", sm)
    sv2, sm2 = sub_success_lines([])
    check("sub success no codes verdict", sv2 == "subtitles - none available", sv2)
    check("sub success no codes miss note",
          sm2 == "note: yt-dlp reported success but no subtitle files were read "
                 "- wording may have changed, please report", sm2)
    fl = sub_fail_lines(r429_src)
    check("sub fail lines verdict", fl[0] == "subtitles - not fetched (video kept). "
          "unable to download subtitles", fl[0])
    check("sub fail lines classified reason",
          fl[1] == "subtitle error - the subtitle api answered 429 too many requests", fl[1])
    fl2 = sub_fail_lines("SOMETHING ODD")
    check("sub fail lines parse-miss note",
          fl2[1] == "subtitle error - yt-dlp wording not recognized - please report", fl2[1])

    s = load_settings()
    d = json.loads(json.dumps(DEFAULTS))
    check("load_settings fills missing keys", all(k in s for k in d))
    tmp = json.loads(json.dumps(d))
    tmp["video_container"] = "mkv"
    fd = Path("FFMPEG_DIR_SENTINEL")

    a = build_args(d, "https://x/y", fd)
    check("video1080 fmt", a[a.index("-f") + 1] == "bv*[height<=1080]+ba/b", a)
    check("video1080 merge mp4", "--merge-output-format" in a and a[a.index("--merge-output-format") + 1] == "mp4", a)
    check("no-playlist default", "--no-playlist" in a, a)
    check("ffmpeg location passed", "--ffmpeg-location" in a, a)
    check("url last", a[-1] == "https://x/y", a)

    tmp["video_resolution"] = "best"
    a = build_args(tmp, "https://x/y", fd)
    check("best fmt", a[a.index("-f") + 1] == "bv*+ba/b", a)

    tmp["video_resolution"] = "720"
    tmp["video_container"] = "best"
    a = build_args(tmp, "https://x/y", fd)
    check("no merge when container best", "--merge-output-format" not in a, a)

    tmp["type"] = "audio only"
    a = build_args(tmp, "https://x/y", fd)
    check("audio extract", "-x" in a and a[a.index("--audio-format") + 1] == "mp3", a)
    check("audio quality", a[a.index("--audio-quality") + 1] == "0", a)

    a = build_args(tmp, "https://x/y", None)
    check("no ffmpeg -> bestaudio", a[a.index("-f") + 1] == "bestaudio", a)
    check("no ffmpeg -> no -x", "-x" not in a, a)

    tmp["type"] = "video"
    a = build_args(tmp, "https://x/y", None)
    check("no ffmpeg video -> -f b", a[a.index("-f") + 1] == "b", a)
    check("no ffmpeg -> no ffmpeg-location", "--ffmpeg-location" not in a, a)

    tmp["type"] = "video no audio"
    a = build_args(tmp, "https://x/y", None)
    check("video no audio 720 fmt", a[a.index("-f") + 1] == "bv*[height<=720]", a)
    check("video no audio no merge", "--merge-output-format" not in a, a)
    check("video no audio no -x", "-x" not in a, a)
    tmp["type"] = "video"

    tmp["playlist"] = True
    tmp["playlist_max_items"] = 3
    a = build_args(tmp, "https://x/y", fd)
    check("playlist items", "--playlist-items" in a and a[a.index("--playlist-items") + 1] == "1-3", a)
    check("playlist drops no-playlist", "--no-playlist" not in a, a)

    tmp["subtitles"] = True
    a = build_args(tmp, "https://x/y", fd)
    check("video run carries no subs flags", "--sub-langs" not in a
          and "--write-subs" not in a, a)
    sa = build_sub_args(tmp, "https://x/y", fd)
    check("sub args skip download", "--skip-download" in sa, sa)
    check("sub args lang pattern", "--sub-langs" in sa
          and a_index(sa, "--sub-langs") == "en(-.*)?", sa)
    check("sub args convert srt with ffmpeg", a_index(sa, "--convert-subs") == "srt", sa)
    sa = build_sub_args(tmp, "https://x/y", None)
    check("sub args vtt without ffmpeg", a_index(sa, "--convert-subs") == "vtt", sa)
    check("sub args share output template", a_index(sa, "-o") == a_index(a, "-o"), sa)
    check("lang pattern multi", sub_lang_patterns("en,nl") == "en(-.*)?,nl(-.*)?",
          sub_lang_patterns("en,nl"))
    check("lang pattern passthrough", sub_lang_patterns("en.*") == "en.*",
          sub_lang_patterns("en.*"))

    cp = color_progress("15.2% of 198.15MiB at 2.32MiB/s ETA 01:12")
    check("progress percent amber", A_AMBER + "15.2%" + A_RESET in cp, cp)
    check("progress size one decimal", A_MINT + "198.1MiB" + A_RESET in cp, cp)
    check("progress speed padded", A_LBLUE + "002.3MiB/s" + A_RESET in cp, cp)
    check("progress eta purple", A_PURPLE + "01:12" + A_RESET in cp, cp)
    check("progress words grey", A_GREY + "of" + A_RESET in cp
          and A_GREY + "at" + A_RESET in cp
          and A_GREY + "ETA" + A_RESET in cp, cp)
    check("progress final line",
          A_AMBER + "100%" + A_RESET
          in color_progress("100% of 198.15MiB in 00:45"))
    cp2 = color_progress("5.0% of ~4.63MiB at 9.94MiB/s ETA 00:57")
    check("speed pads the 9 to 10 crossing",
          A_LBLUE + "009.9MiB/s" + A_RESET in cp2
          and A_MINT + "~4.6MiB" + A_RESET in cp2, cp2)
    check("speed keeps three digits unpadded",
          pad_speed("999.9MiB/s") == "999.9MiB/s", pad_speed("999.9MiB/s"))
    check("speed truncates one decimal",
          pad_speed("99.99MiB/s") == "099.9MiB/s", pad_speed("99.99MiB/s"))
    check("percent never gains a digit",
          one_dec_num("15.2%") == "15.2%", one_dec_num("15.2%"))

    check("eight spinners", len(SPINNERS) == 8, [s[0] for s in SPINNERS])
    check("spinner names unique", len(set(s[0] for s in SPINNERS)) == 8,
          [s[0] for s in SPINNERS])
    for sname, sframes, stick, sfreeze in SPINNERS:
        ok = (len(sframes) > 0 and 0 < stick <= 0.2
              and all(len(f) == 6 for f in sframes)
              and all(isinstance(ch, str) and isinstance(col, int)
                      and 0 <= col <= 255
                      for f in sframes for ch, col in f)
              and len(sfreeze) == 6)
        check("spinner %s shape" % sname, ok, sname)
    pick = pick_spinner()
    check("spinner pick is one of them", pick in SPINNERS, pick[0])
    check("render_cells one color run",
          render_cells([("@", 231), (".", 231)])
          == "\033[38;5;231m@.\033[0m",
          render_cells([("@", 231), (".", 231)]))
    check("render_cells color change",
          render_cells([("@", 231), (".", 202)])
          == "\033[38;5;231m@\033[38;5;202m.\033[0m",
          render_cells([("@", 231), (".", 202)]))
    check("plain_cells strips padding",
          plain_cells(SPINNERS[0][3]) == "@", plain_cells(SPINNERS[0][3]))
    check("flat_frames cell layout",
          flat_frames([".  "], 214)
          == [[(".", 214), (" ", 214), (" ", 214)]],
          flat_frames([".  "], 214))
    check("flat freeze stays plain",
          plain_cells(flat_freeze("...", 214)) == "...",
          flat_freeze("...", 214))
    ds = dot_stage("probe stage")
    check("dot stage carries amber dots",
          ds.frames == flat_frames(DOTS, 214) and ds.tick == FRAME_SECS,
          ds.frames)
    check("dot stage renders in color",
          render_cells(ds.freeze) == "\033[38;5;214m...\033[0m",
          render_cells(ds.freeze))
    check("spinner freeze renders in color",
          all(render_cells(f).endswith(A_RESET) for f in SPINNERS[0][1]),
          render_cells(SPINNERS[0][1][0]))

    tmp["subtitles"] = False
    a = build_args(tmp, "https://x/y", None)
    check("video run never has convert-subs", "--convert-subs" not in a, a)

    tmp["filename_template"] = "C:/out/%(title)s.%(ext)s"
    a = build_args(tmp, "https://x/y", fd)
    check("absolute template -> no -P", "-P" not in a, a)

    tmp["filename_template"] = "play/%(title)s.%(ext)s"
    a = build_args(tmp, "https://x/y", fd)
    check("relative template keeps -P", a[a.index("-P") + 1].startswith("home:"), a)

    cwd = str(Path.cwd())
    fixed = json.loads(json.dumps(d))
    fixed["output_dir"] = cwd
    check("resolve_outdir keeps saved folder",
          str(resolve_outdir(fixed)) == cwd, resolve_outdir(fixed))
    auto = auto_outdir()
    check("auto_outdir is an existing folder or None",
          auto is None or os.path.isdir(auto), auto)
    empty = json.loads(json.dumps(d))
    empty["output_dir"] = ""
    a = build_args(empty, "https://x/y", fd)
    check("empty output_dir still passes -P home",
          a[a.index("-P") + 1].startswith("home:"), a)

    check("clean_url basic", clean_url("watch https://youtu.be/abc-123_x now") == "https://youtu.be/abc-123_x", clean_url("watch https://youtu.be/abc-123_x now"))
    check("clean_url trims punctuation", clean_url("https://youtu.be/z9?si=1, ok") == "https://youtu.be/z9?si=1", clean_url("https://youtu.be/z9?si=1, ok"))
    check("clean_url none", clean_url("no link here") is None, clean_url("no link here"))

    long_err = ("ERROR: unable to rename file: [WinError 32] The process cannot"
                " access the file because it is being used by another process")
    check("fail sentence has rc", fail_sentence(7, "") == "downloading - failed (rc=7)", fail_sentence(7, ""))
    check("fail sentence keeps whole error line",
          fail_sentence(1, long_err) == "downloading - failed (rc=1): " + long_err,
          fail_sentence(1, long_err))
    check("fail sentence no width cut", len(fail_sentence(1, long_err)) > WIDTH, len(fail_sentence(1, long_err)))

    rows = menu_items(load_settings())
    nums = [r[0] for r in rows]
    check("menu has 13 rows", len(rows) == 13 and nums == list(range(1, 14)), nums)
    check("menu row 9 is multi url",
          rows[8][1] == "add multiple youtube urls", rows[8])
    check("menu row 13 is quit", rows[12][1] == "quit", rows[12])
    check("menu row 9 has no value",
          rows[8][2] == "" and rows[8][3] is True, rows[8])
    check("menu rows 10 to 12 shifted",
          rows[9][1] == "reset defaults"
          and rows[10][1] == "update yt-dlp + cli"
          and rows[11][1] == "auto update", (rows[9], rows[10], rows[11]))
    check("auto update row shows its state",
          rows[11][2] in ("on", "off"), rows[11])

    check("auto update default is on",
          DEFAULTS["auto_update"] is True, DEFAULTS)
    check("auto update stamps its last check",
          "auto_update_last_check" in DEFAULTS, sorted(DEFAULTS))
    check("ffmpeg sources cover windows and linux",
          sorted(FFMPEG_SOURCES) == ["linux", "windows"], sorted(FFMPEG_SOURCES))
    check("no ffmpeg download for macos",
          "macos" not in FFMPEG_SOURCES, sorted(FFMPEG_SOURCES))
    for _p, (_u, _s, _b, _h) in sorted(FFMPEG_SOURCES.items()):
        check("ffmpeg source %s is well formed" % _p,
              _u.startswith("https://") and _s.startswith("https://")
              and _b in ("ffmpeg", "ffmpeg.exe") and _h in ("zip", "tar.xz"),
              (_u, _s, _b, _h))
    check("windows ffmpeg source is gyan.dev",
          "gyan.dev" in FFMPEG_SOURCES["windows"][0],
          FFMPEG_SOURCES["windows"][0])
    check("linux ffmpeg source is BtbN",
          "BtbN/FFmpeg-Builds" in FFMPEG_SOURCES["linux"][0],
          FFMPEG_SOURCES["linux"][0])
    check("cli update url is raw main branch",
          CLI_UPDATE_URL.endswith("/main/app-python/src/ytdlp.py")
          and "raw.githubusercontent" in CLI_UPDATE_URL, CLI_UPDATE_URL)

    check("version_tuple compares numerically",
          version_tuple("1.10.0") > version_tuple("1.9.0")
          and version_tuple("1.2.0") < version_tuple("1.3")
          and version_tuple("") == (0,)
          and version_tuple("1.3") == (1, 3),
          (version_tuple("1.10.0"), version_tuple("1.9")))
    check("version_tuple counts junk pieces as 0",
          version_tuple("1.3-beta") >= version_tuple("1.3")
          and version_tuple("dev") == (0,), version_tuple("1.3-beta"))
    check("parse_sha256 reads a bare 64 char digest",
          parse_sha256("60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba")
          == "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba")
    check("parse_sha256 reads digest then filename",
          parse_sha256("60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba"
                       "  ffmpeg-9.0.2.zip")
          == "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba")
    check("parse_sha256 refuses anything without a digest",
          parse_sha256("no digest here") is None
          and parse_sha256("") is None
          and parse_sha256("tooshort") is None
          and parse_sha256("g" * 64) is None)
    check("pick_checksum picks the line naming the asset",
          pick_checksum("aaa  x.zip\n"
                        "afa2b69c9965919e0b519a633520321f838d74690f8fb28c76393c54a9448d60"
                        "  ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz\nbbb  y.zip\n",
                        "ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz")
          == "afa2b69c9965919e0b519a633520321f838d74690f8fb28c76393c54a9448d60")
    check("pick_checksum misses an asset it was not given",
          pick_checksum("aaa  x.zip\nbbb  y.zip\n", "z.zip") is None
          and pick_checksum("", "z.zip") is None)

    check("install_cli refuses a file that does not parse",
          install_cli("def broken(:\n", CLI_SELF_PATH)[0] is False)
    check("install_cli wrote nothing when it refused",
          not CLI_SELF_PATH.with_name(CLI_SELF_PATH.name + ".new").exists())
    check("install_cli writes exactly the bytes it was given",
          install_cli('VERSION = "9.9.9"\n',
                      CLI_SELF_PATH.with_name(CLI_SELF_PATH.name + ".new"))[0]
          is True
          and CLI_SELF_PATH.with_name(CLI_SELF_PATH.name + ".new")
          .read_text(encoding="utf-8") == 'VERSION = "9.9.9"\n')
    CLI_SELF_PATH.with_name(CLI_SELF_PATH.name + ".new").unlink()

    import io
    _ex = Path(tempfile.mkdtemp(prefix="ytdlp-st-"))
    try:
        _zd, _td, _ed = _ex / "z", _ex / "t", _ex / "e"
        for _d in (_zd, _td, _ed):
            _d.mkdir()
        _zip = _ex / "a.zip"
        with zipfile.ZipFile(_zip, "w") as zf:
            zf.writestr("ffmpeg-9.0.2-essentials_build/bin/ffmpeg.exe", "BINARY")
            zf.writestr("ffmpeg-9.0.2-essentials_build/bin/ffplay.exe", "OTHER")
        _got = extract_member(_zip, "zip", "ffmpeg.exe", _zd)
        check("extract_member lifts the nested binary from a zip",
              _got is not None and _got.read_bytes() == b"BINARY", _got)
        check("extract_member takes only the named binary",
              not (_zd / "ffplay.exe").exists(), list(_zd.iterdir()))
        _tar = _ex / "a.tar.xz"
        _data = b"TARDATA"
        _info = tarfile.TarInfo("ffmpeg-n9.0-linux64-gpl-9.0/bin/ffmpeg")
        _info.size = len(_data)
        with tarfile.open(_tar, "w:xz") as tf:
            tf.addfile(_info, io.BytesIO(_data))
        _got2 = extract_member(_tar, "tar.xz", "ffmpeg", _td)
        check("extract_member lifts the nested binary from a tar.xz",
              _got2 is not None and _got2.read_bytes() == _data, _got2)
        _bad = _ex / "bad.zip"
        _bad.write_bytes(b"this is not a zip")
        check("extract_member reports an unreadable archive",
              extract_member(_bad, "zip", "ffmpeg.exe", _ed) is None)
        _miss = _ex / "m.zip"
        with zipfile.ZipFile(_miss, "w") as zf:
            zf.writestr("ffmpeg-9.0.2/bin/ffplay.exe", "x")
        check("extract_member returns None when the binary is absent",
              extract_member(_miss, "zip", "ffmpeg.exe", _ed) is None)
        check("extract_member refuses an unknown format",
              extract_member(_zip, "rar", "ffmpeg.exe", _ed) is None)

        # the exact bug this code shipped with: ffmpeg 9 rejects "--version",
        # prints its banner to stderr and exits 0xABAFB008, then answers
        # "-version" on stdout with rc 0. probe_version must not read that as
        # a broken binary.
        _sd = _ex / "shim"
        _sd.mkdir()
        if os.name == "nt":
            _shim = _sd / "ff9.bat"
            _shim.write_text(
                "@echo off\r\n"
                "if \"%~1\"==\"--version\" echo ffmpeg version 9.0.2 fake 1>&2"
                " & exit /b 2880417800\r\n"
                "echo ffmpeg version 9.0.2 fake\r\nexit /b 0\r\n",
                encoding="ascii")
        else:
            _shim = _sd / "ff9.sh"
            _shim.write_text(
                "#!/bin/sh\n"
                "if [ \"$1\" = \"--version\" ]; then\n"
                "  echo 'ffmpeg version 9.0.2 fake' >&2\n"
                "  exit 2880417800\n"
                "fi\n"
                "echo 'ffmpeg version 9.0.2 fake'\n"
                "exit 0\n",
                encoding="ascii")
            os.chmod(_shim, 0o755)
        _v, _rc = probe_version(_shim)
        check("probe_version survives a binary that rejects --version",
              _rc == 0 and "9.0.2" in _v, (_v, _rc))
    finally:
        shutil.rmtree(_ex, ignore_errors=True)

    langs = sub_lang_lines()
    check("lang lines fit the box",
          langs and all(len(l) <= WIDTH for l in langs), langs)
    check("lang lines are indented and spaced",
          all(l.startswith("  ") and "  " not in l.strip() and l == l.rstrip()
              for l in langs), langs)
    flat = [w for l in langs for w in l.split()]
    check("lang list carries the expected codes",
          "en" in flat and "lt" in flat and "et" in flat and "ru" in flat
          and "zh" in flat and "vi" in flat and "ja" in flat and "ko" in flat,
          langs)
    check("lang codes are two letters, fil aside",
          all(w.isalpha() and 2 <= len(w) <= 3 and w.islower() for w in flat)
          and "fil" in flat, [w for w in flat if not w.isalpha()])
    check("lang first line fills the box", len(langs[0]) == WIDTH,
          len(langs[0]))
    check("lang list is the whole reference, alphabetical",
          flat == sorted(KNOWN_LANG_CODES) and len(flat) == 191,
          (len(flat), len(KNOWN_LANG_CODES)))
    check("lang list fits in ten lines", len(langs) == 10, len(langs))
    check("lang lines wrap narrow", all(len(l) <= 24 for l in sub_lang_lines(24)),
          sub_lang_lines(24))

    check("lang ref is what the list shows",
          set(flat) == set(KNOWN_LANG_CODES)
          and all(len(w) == 2 or w == "fil" for w in KNOWN_LANG_CODES),
          sorted(set(flat) ^ set(KNOWN_LANG_CODES)))
    check("lang ref has no yt-dlp-only leftovers",
          not [w for w in KNOWN_LANG_CODES if len(w) > 3],
          sorted(KNOWN_LANG_CODES))
    k1, b1, u1 = parse_lang_input("en,8")
    check("digit dropped, en kept", k1 == ["en"] and b1 == ["8"] and not u1,
          (k1, b1, u1))
    k2, b2, u2 = parse_lang_input("yue,nl-BE")
    check("unknown kept, variant kept",
          k2 == ["yue", "nl-be"] and not b2 and u2 == ["yue"], (k2, b2, u2))
    k3, b3, u3 = parse_lang_input("en-GB, sr-Latn, zh-Hans, es-419")
    check("variants all kept", k3 == ["en-gb", "sr-latn", "zh-hans", "es-419"]
          and not b3 and not u3, (k3, b3, u3))
    k4, b4, u4 = parse_lang_input("  EN , , de  ")
    check("spaces, case and empties",
          k4 == ["en", "de"] and not b4 and not u4, (k4, b4, u4))
    k5, b5, u5 = parse_lang_input("en.*,nl,xx!!")
    check("wildcard passes through", k5[0] == "en.*" and b5 == ["xx!!"]
          and not u5, (k5, b5, u5))
    k5b, b5b, u5b = parse_lang_input("fil,yue")
    check("fil known, yue kept with note",
          k5b == ["fil", "yue"] and not b5b and u5b == ["yue"], (k5b, b5b, u5b))
    k6, b6, u6 = parse_lang_input("8")
    check("only digits leaves nothing", not k6 and b6 == ["8"] and not u6,
          (k6, b6, u6))

    # item 7 paints the block under the input line. the selftest is not a tty so
    # block_under_input is False here and the safe fallback (block above the
    # prompt) is what runs; the under-input path is covered by the escape
    # checks below.
    order = []
    g = globals()
    real_esc = g["read_esc"]
    real_color = g["USE_COLOR"]

    def fake_esc(prompt, below=None):
        order.append((prompt, below))
        if prompt:                       # read_esc writes the prompt first
            sys.stdout.write(prompt)
        if below:
            sys.stdout.write("\n" + "\n".join(below) + "\n")
            sys.stdout.write("\033[%dA" % (len(below) + 1))
        if len(order) == 1:
            sys.stdout.write("y\n")      # echo inline, newline on Enter
            return "y"
        if len(order) == 2:
            sys.stdout.write("en,8\n")
            return "en,8"
        sys.stdout.write("\n")           # the unlabelled hold
        return ""

    g["read_esc"] = fake_esc
    g["USE_COLOR"] = True
    cap3 = io.StringIO()
    old3 = sys.stdout
    sys.stdout = cap3
    try:
        rep7 = act("7", load_settings())
    finally:
        sys.stdout = old3
        g["read_esc"] = real_esc
        g["USE_COLOR"] = real_color
    plain7 = [l.replace(A_AMBER, "").replace(A_DARKGREY, "")
              .replace(A_RESET, "") for l in cap3.getvalue().splitlines()]
    block7 = [l.strip() for l in sub_lang_lines()]
    nb = len(block7)
    check("item 7 fallback puts the block above the input",
          len(order) == 3 and order[1] == ("", None)
          and block_under_input(block7) is False
          and [l.strip() for l in plain7[1:1 + nb]] == block7,
          (len(order), plain7[:2]))
    check("item 7 label gets its own typing line",
          len(order) == 3 and order[1] == ("", None)
          and plain7[1 + nb].endswith("(enter = keep):")
          and plain7[2 + nb] == "en,8", plain7[1 + nb:3 + nb])
    check("item 7 verdict follows the typed value",
          plain7[3 + nb] == "  not a language code, dropped: 8"
          and plain7[4 + nb] == "", plain7[3 + nb:])

    # the under-input path: the block is written, then the caret is moved back
    # up to the prompt line, and the amber rewinds one row and repaints it.
    class FakeStdin:
        @staticmethod
        def isatty():
            return True

    real_stdin = g["sys"].stdin
    real_size = os.get_terminal_size
    g["USE_COLOR"] = True
    g["sys"].stdin = FakeStdin
    os.get_terminal_size = lambda: os.terminal_size((120, 50))
    try:
        roomy = block_under_input(sub_lang_lines())
        os.get_terminal_size = lambda: os.terminal_size((120, 12))
        tight = block_under_input(sub_lang_lines())
        os.get_terminal_size = lambda: os.terminal_size((60, 50))
        narrow = block_under_input(sub_lang_lines())
        cap4 = io.StringIO()
        old4 = sys.stdout
        sys.stdout = cap4
        try:
            sys.stdout.write("  enter languages: ")
            sys.stdout.write("\n" + "\n".join(["  x"] * 3) + "\n")
            sys.stdout.write("\033[%dA" % 4)
        finally:
            sys.stdout = old4
    finally:
        g["USE_COLOR"] = real_color
        g["sys"].stdin = real_stdin
        os.get_terminal_size = real_size
    painted = cap4.getvalue()
    check("block fits under a live cursor when the window is roomy",
          roomy is True, roomy)
    check("block under input ends with the cursor back on the prompt",
          painted.endswith("\033[4A") and painted.count("\n") == 4, repr(painted))
    check("a block taller than the window falls back", tight is False, tight)
    check("a block wider than the window falls back", narrow is False, narrow)

    # every message printed after a submenu's last read has to hold, or the
    # menu repaint eats it. this bit twice live, so pin every call site.
    held = []

    def held_esc(prompt=""):
        held.append(prompt)
        return ""

    # run each case under capture, then report with stdout restored, so a
    # FAIL line is never swallowed into the capture
    def run_pick(typed):
        held.clear()
        cap = io.StringIO()
        old = sys.stdout
        g["read_esc"] = lambda prompt="", _t=typed: (
            held.append(prompt), _t)[1]
        sys.stdout = cap
        try:
            got = pick_list("t", ["a", "b"], "a")
        finally:
            sys.stdout = old
            g["read_esc"] = real_esc
        return got, list(held), cap.getvalue()

    got_bad, hs_bad, out_bad = run_pick("abc")
    check("pick_list holds on invalid number",
          got_bad == "a" and len(hs_bad) == 2 and hs_bad[-1] == ""
          and "invalid number" in out_bad, (hs_bad, out_bad))
    got_range, hs_range, out_range = run_pick("9")
    check("pick_list holds on out of range",
          got_range == "a" and len(hs_range) == 2 and hs_range[-1] == ""
          and "out of range" in out_range, (hs_range, out_range))
    got_ok, hs_ok, out_ok = run_pick("2")
    check("pick_list does not hold on a good pick",
          got_ok == "b" and len(hs_ok) == 1 and hs_ok[0].strip()
          and "range" not in out_ok and "invalid" not in out_ok,
          (hs_ok, out_ok))

    # item 8 and item 10 print after their last read and must hold as well
    for choice, replies, expect in (("8", ["y", "abc"], "not a number - kept"),
                                   ("10", [], "defaults restored")):
        got_replies = list(replies)
        prompts = []

        def queued(prompt="", _r=got_replies, _p=prompts):
            _p.append(prompt)
            return _r.pop(0) if _r else ""

        st = load_settings()
        st["playlist"] = True
        g["read_esc"] = queued
        cap6 = io.StringIO()
        old6 = sys.stdout
        sys.stdout = cap6
        try:
            act(choice, st)
        finally:
            sys.stdout = old6
            g["read_esc"] = real_esc
        body6 = cap6.getvalue()
        check("item %s holds its message" % choice,
              expect in body6 and prompts[-1:] == [""], (body6[-60:], prompts))

    check("batch row keeps url", batch_row(1, "https://x/y").endswith("https://x/y"),
          batch_row(1, "https://x/y"))
    check("batch row marks bad entry",
          batch_row(2, "nope") == c(A_GREY, "  2  ") + c(A_AMBER, "error in url"),
          batch_row(2, "nope"))

    seen = []

    def fake(s2, url):
        seen.append(url)
        return 0

    cap2 = io.StringIO()
    sys.stdout = cap2
    try:
        ran = run_batch(load_settings(),
                        ["https://a/1", "not a url", "https://b/2"], fake)
        body = cap2.getvalue()
    finally:
        sys.stdout = real_out
    check("batch skips bad entry, keeps order",
          seen == ["https://a/1", "https://b/2"], seen)
    check("batch marks bad entry in run", "error in url" in body, body)
    check("batch reports position",
          "url 2 of 3" in body and "url 3 of 3" in body, body)
    check("batch ran -> no repaint", ran is True, ran)

    attempts = []

    def stopper(s2, url):
        attempts.append(url)
        return 130

    cap3 = io.StringIO()
    sys.stdout = cap3
    try:
        stopped = run_batch(load_settings(),
                            ["https://a/1", "https://b/2"], stopper)
    finally:
        sys.stdout = real_out
    check("batch stops on escape",
          stopped is False and attempts == ["https://a/1"], (stopped, attempts))

    cap4 = io.StringIO()
    sys.stdout = cap4
    try:
        hint(load_settings())
        hbody = cap4.getvalue()
        r13 = act("14", d)
        abody = cap4.getvalue()
    finally:
        sys.stdout = real_out
    check("hint points at item 9", "menu item: 9" in hbody, hbody)
    check("act rejects 14 with new range",
          "1-13" in abody and r13 is True, (abody, r13))

    # "9" must reach multi_url: a mis-dispatch there would reset every default
    hit = []

    def fake_multi(s2):
        hit.append("multi")
        return False

    real_multi = globals()["multi_url"]
    globals()["multi_url"] = fake_multi
    try:
        rep9 = act("9", d)
    finally:
        globals()["multi_url"] = real_multi
    check("act 9 is multi url", hit == ["multi"] and rep9 is False, (hit, rep9))

    # ---- audit fix regressions (2026-10-14) ----

    # F1/F2: junk hand-edited settings coerce at the boundary, never raise,
    # never reach argv, and never rewrite the file
    junk = json.loads(json.dumps(DEFAULTS))
    junk["playlist"] = True
    junk["playlist_max_items"] = "abc"
    junk["video_resolution"] = "not-a-number"
    junk["audio_quality"] = "banana"
    junk["video_container"] = "avi"
    junk["type"] = "tunnel"
    settings_before = CONFIG_PATH.read_bytes() if CONFIG_PATH.exists() else b""
    capj = io.StringIO()
    sys.stdout = capj
    try:
        coerce_settings(junk)
        mj = menu_items(junk)
        aj = build_args(junk, "https://x/y", Path("FFMPEG_DIR_SENTINEL"))
    finally:
        sys.stdout = real_out
    jargv = " ".join(aj)
    check("coerce fixes junk max_items",
          junk["playlist_max_items"] == 0, junk["playlist_max_items"])
    check("coerce keeps junk out of argv",
          "height<=not-a-number" not in jargv and "banana" not in jargv, jargv)
    check("coerce fixes junk vocabularies",
          junk["video_resolution"] == "1080" and junk["video_container"] == "mp4"
          and junk["audio_format"] == "mp3" and junk["type"] == "video", junk)
    check("coerce renders the menu without raising", len(mj) == 13, len(mj))
    check("coerce never rewrites settings.json",
          (CONFIG_PATH.read_bytes() if CONFIG_PATH.exists() else b"")
          == settings_before, CONFIG_PATH)
    q9 = json.loads(json.dumps(DEFAULTS))
    q9["audio_quality"] = "9"
    coerce_settings(q9)
    check("coerce keeps number quality", q9["audio_quality"] == "9", q9["audio_quality"])
    q320 = json.loads(json.dumps(DEFAULTS))
    q320["audio_quality"] = "320K"
    coerce_settings(q320)
    check("coerce keeps bitrate quality", q320["audio_quality"] == "320K", q320["audio_quality"])
    n5 = json.loads(json.dumps(DEFAULTS))
    n5["playlist_max_items"] = "5"
    coerce_settings(n5)
    check("coerce parses numeric string max_items",
          n5["playlist_max_items"] == 5, n5["playlist_max_items"])

    # F3: the non-forced daily check asks for the short timeout, forced keeps 60
    got_timeouts = []

    def fake_fetch(url=CLI_UPDATE_URL, timeout=60):
        got_timeouts.append(timeout)
        raise Exception("offline")

    g = globals()
    real_fetch, real_ask = g["fetch_cli"], g["can_ask"]
    g["fetch_cli"], g["can_ask"] = fake_fetch, (lambda: True)
    s_auto = json.loads(json.dumps(DEFAULTS))
    capf = io.StringIO()
    sys.stdout = capf
    try:
        auto_update_check(s_auto)
        auto_update_check(s_auto, force=True)
    finally:
        sys.stdout = real_out
        g["fetch_cli"], g["can_ask"] = real_fetch, real_ask
    check("auto check uses the pinned 6s timeout",
          got_timeouts[:1] == [CHECK_TIMEOUT], got_timeouts)
    check("forced check keeps the 60s timeout",
          got_timeouts[1:] == [60], got_timeouts)

    # F4: a second probe of the same binary must not spawn it again
    calls = []
    real_run = subprocess.run

    def counting_run(argv, **kw):
        calls.append(argv)
        return real_run(argv, **kw)

    subprocess.run = counting_run
    try:
        probe_version(sys.executable)
        spawns_first = len(calls)
        probe_version(sys.executable)
        spawns_second = len(calls)
    finally:
        subprocess.run = real_run
    check("probe memo skips the second spawn",
          spawns_first in (1, 2) and spawns_second == spawns_first,
          (spawns_first, spawns_second))

    # F6: the OS lookup runs once, repaints and downloads reuse the memo
    real_lookup = g["_auto_outdir_lookup"]
    lookups = []

    def counting_lookup():
        lookups.append(1)
        return real_lookup()

    g["_auto_outdir_lookup"] = counting_lookup
    g["_outdir_cache"].clear()
    try:
        a1 = auto_outdir()
        a2 = auto_outdir()
    finally:
        g["_auto_outdir_lookup"] = real_lookup
    check("auto_outdir memo skips the second lookup",
          len(lookups) == 1 and a1 == a2, (len(lookups), a1, a2))

    # F8: item 12 leaves the save to the one save point in main_menu
    saves = []
    real_save = g["save_settings"]
    real_esc2 = g["read_esc"]
    g["save_settings"] = lambda s2: saves.append("save")
    g["read_esc"] = lambda prompt="", **kw: ""
    try:
        act("12", d)
    finally:
        g["save_settings"], g["read_esc"] = real_save, real_esc2
    check("item 12 leaves the save to main_menu", saves == [], saves)

    # F10: wide code points count columns, and the box row keeps its width
    wide = chr(0x4E00)
    check("dwidth counts wide chars as two columns",
          dwidth("ab") == 2 and dwidth(wide) == 2 and dwidth("a" + wide) == 3,
          (dwidth("ab"), dwidth(wide)))
    check("cut_to_width cuts by columns",
          cut_to_width("ab" + wide * 3, 6) == "ab" + wide * 2
          and cut_to_width("ab", 1) == "a",
          (cut_to_width("ab" + wide * 3, 6), cut_to_width("ab", 1)))
    check("tail_to_width keeps the tail by columns",
          tail_to_width("ab" + wide * 3, 6) == wide * 3, tail_to_width("ab" + wide * 3, 6))
    ansi = re.compile("\x1b\\[[0-9;]*m")
    base_row = ansi.sub("", menu_line(1, "x", "plain"))
    wide_row = ansi.sub("", menu_line(1, "x", wide * 100))
    check("menu line keeps box width with a wide value",
          dwidth(base_row) == dwidth(wide_row) == WIDTH + 2,
          (dwidth(base_row), dwidth(wide_row)))

    print("-" * WIDTH)
    print("selftest: %d checks, %d failed" % (count[0], len(fails)))
    return 1 if fails else 0


def apply_overrides(s, args):
    if args.audio:
        s["type"] = "audio only"
    if args.format:
        s["video_container"] = args.format
    if args.resolution:
        s["video_resolution"] = args.resolution
    if args.af:
        s["audio_format"] = args.af
    if args.outdir:
        s["output_dir"] = args.outdir
    if args.subs:
        s["subtitles"] = True
    if args.playlist:
        s["playlist"] = True
    if args.template:
        s["filename_template"] = args.template


def main(argv=None):
    p = argparse.ArgumentParser(description="yt-dlp CLI wrapper")
    p.add_argument("url", nargs="?", help="download this URL with saved defaults")
    p.add_argument("-a", "--audio", action="store_true", help="audio only")
    p.add_argument("-f", "--format", choices=CONTAINERS, help="container")
    p.add_argument("-r", "--resolution", choices=RESOLUTIONS, help="max height")
    p.add_argument("--af", choices=AUDIO_FORMATS, help="audio format")
    p.add_argument("-o", "--outdir", help="output folder")
    p.add_argument("--template", help="output filename template")
    p.add_argument("--subs", action="store_true", help="subtitles")
    p.add_argument("--playlist", action="store_true", help="playlist mode")
    p.add_argument("--menu", action="store_true", help="force menu even with URL")
    p.add_argument("--check", action="store_true", help="show resolved paths/versions")
    p.add_argument("--update", action="store_true", help="update the yt-dlp binary in place")
    p.add_argument("--selftest", action="store_true", help="run built-in logic tests")
    args = p.parse_args(argv)

    s = load_settings()
    apply_overrides(s, args)

    if args.check:
        show_check(s)
        return 0
    if args.selftest:
        return selftest()
    if args.update:
        return 0 if update_all(s) else 1
    if args.url and not args.menu:
        return do_download(s, args.url)
    return main_menu(s)


if __name__ == "__main__":
    sys.exit(main())
