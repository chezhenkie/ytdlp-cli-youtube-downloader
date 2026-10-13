#!/usr/bin/env python3
"""Interactive CLI and quick launcher around yt-dlp.

Menu mode:   python app-python/src/ytdlp.py
Quick mode:  python app-python/src/ytdlp.py <url> [-a] [-f mp4] [-r 1080] [--subs]
Checks:      python app-python/src/ytdlp.py --check | --selftest

Stdlib only. Windows / Linux / macOS. Defaults persist in app-python/config/settings.json.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

VERSION = "1.2"
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


def color_progress(body):
    """Color one [download] body: four numbers, four colors.

    Percent amber 214, file size mint green 121, speed light blue 117, ETA
    desaturated purple 176; the plain words ("of", "at", "in", "ETA") are
    grey 250. Pure function, testable without a tty; off a tty the caller
    skips it entirely."""
    out = []
    for word in body.split(" "):
        if "%" in word:
            out.append(A_AMBER + word + A_RESET)
        elif SIZE_RE.match(word):
            out.append(A_MINT + word + A_RESET)
        elif SPEED_RE.match(word):
            out.append(A_LBLUE + word + A_RESET)
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


def read_esc(prompt=""):
    """One input line; returns None if Escape was pressed.

    Raw console reader so escape is detectable. Falls back to input() when
    stdin is not a tty (pipes, tests)."""
    if not sys.stdin.isatty():
        return input(prompt)
    if prompt:
        sys.stdout.write(prompt)
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
    "type": "video",
    "video_container": "mp4",
    "video_resolution": "1080",
    "audio_format": "mp3",
    "audio_quality": "0",
    # empty = auto-resolve: OS desktop, fallback OS download folder
    "output_dir": "",
    "filename_template": "%(title)s",
    "subtitles": False,
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
# reference codes for the subtitles menu: europe first (baltics are
# european), then the rest. two-letter codes only, the ones you type back
# into the languages prompt.
SUB_LANG_CODES = (
    "en de fr es it pt nl sv da no fi is pl cs sk hu ro el bg hr sr sl "
    "lv lt et ru zh ko ja id th vi"
)
# base codes youtube itself advertises, taken from the pinned yt-dlp source
# (_SUPPORTED_LANG_CODES in yt_dlp/extractor/youtube/_base.py, 83 codes with
# variants like en-GB and zh-CN, reduced to the 74 base codes). it is a
# reference, not a gate: subtitle codes arrive from captionTracks and youtube
# publishes no full list, so a code missing here is kept, not refused.
YOUTUBE_LANG_CODES = frozenset("""
    af  am  ar  as  az  be  bg  bn  bs  ca  cs  da  de  el  en  es
    et  eu  fa  fi  fil  fr  gl  gu  hi  hr  hu  hy  id  is  it  iw
    ja  ka  kk  km  kn  ko  ky  lo  lt  lv  mk  ml  mn  mr  ms  my
    ne  nl  no  or  pa  pl  pt  ro  ru  si  sk  sl  sq  sr  sv  sw
    ta  te  th  tr  uk  ur  uz  vi  zh  zu
""".split())
# a language code with optional subtags: en, fil, en-GB, sr-Latn, es-419, zh-Hans
LANG_CODE_RE = re.compile(r"^[a-z]{2,3}(?:-[a-z0-9]{2,8})*$")
PLATFORM_BIN = {"windows": "yt-dlp.exe", "linux": "yt-dlp", "macos": "yt-dlp"}


def platform_name():
    if os.name == "nt":
        return "windows"
    if sys.platform.startswith("linux"):
        return "linux"
    if sys.platform == "darwin":
        return "macos"
    return os.name


# ---------------------------------------------------------------- settings

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


def auto_outdir():
    """Default output folder: the OS desktop, fallback the OS download folder.
    Returns None when neither exists (caller uses downloads/ at the project
    root). Desktop first on windows (Shell Folders key) and XDG on unix, with
    ~/Desktop and ~/Downloads as the last candidates; every candidate is
    gated on os.path.isdir so a redirected-but-missing folder falls through."""
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


def probe_version(binary):
    try:
        r = subprocess.run([str(binary), "--version"], capture_output=True, text=True, timeout=60)
        return (r.stdout or "").strip(), r.returncode
    except Exception as exc:
        return "error: %s" % exc, -1


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
    code at all (a bare digit, junk); unknown is a well formed code that our
    reference list does not carry, kept anyway because subtitle codes come
    from youtube per video. Tokens carrying a wildcard pass through, matching
    what sub_lang_patterns already accepts."""
    kept = []
    bad = []
    unknown = []
    for part in str(raw or "").split(","):
        code = part.strip().lower()
        if not code:
            continue
        if any(ch in code for ch in "*?([") or LANG_CODE_RE.match(code):
            kept.append(code)
            if not any(ch in code for ch in "*?([") \
                    and code.split("-")[0] not in YOUTUBE_LANG_CODES:
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

DOTS = [".  ", ".. ", "...", ".. ", ".  "]
CHEVRONS = [">  ", ">> ", ">>>", ">> ", ">  "]
FRAME_SECS = 0.1


class Stage:
    """One animated stage line: a 3-char prefix spins, then freezes.

    done() ends the line with a newline, so finished stages stay on screen
    under the input line until the next url, menu pick, or escape repaints
    the menu. Outside a tty the thread is skipped and only the final line
    prints."""

    def __init__(self, text, frames, color, freeze):
        self.text = text
        self.frames = frames
        self.color = color
        self.freeze = freeze
        self.live = None
        self._stop = False
        self._th = None

    def start(self):
        if not USE_COLOR:
            return
        self._th = threading.Thread(target=self._spin, daemon=True)
        self._th.start()

    def _spin(self):
        i = 0
        while not self._stop:
            if self.live:
                frame, text = self.freeze, self.live
            else:
                frame, text = self.frames[i % len(self.frames)], self.text
                i += 1
            sys.stdout.write("\r%s%s%s %s" % (self.color, frame,
                                              A_RESET, text))
            sys.stdout.flush()
            time.sleep(FRAME_SECS)

    def done(self, text=None):
        self._stop = True
        if self._th is not None:
            self._th.join(timeout=1.0)
        final = text if text is not None else self.text
        if USE_COLOR:
            sys.stdout.write("\r%s%s%s %s\n" % (self.color, self.freeze,
                                                A_RESET, final))
        else:
            print("%s %s" % (self.freeze, final))
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
    st = Stage("checking yt-dlp and ffmpeg", DOTS, A_AMBER, "...")
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

    opts = build_args(s, url, ffdir)
    started = time.time()
    st = Stage("downloading (escape to stop)", CHEVRONS, A_LBLUE, ">>>")
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
        st2 = Stage("subtitles (%s)" % s["subtitle_langs"], DOTS, A_AMBER, "...")
        st2.start()
        rc2, _stopped2, err2, codes = run_download(binary,
                                                    build_sub_args(s, url, ffdir),
                                                    st2)
        if rc2 == 0:
            st2.done("subtitles - %s" % (", ".join(ascii_safe(c2) for c2 in codes) if codes
                                         else "none available"))
        else:
            st2.done("subtitles - not fetched (video kept). "
                     "unable to download subtitles")
            reason = sub_fail_reason(err2)
            if reason:
                st2.done("subtitle error - %s" % reason)
    return 0


# ---------------------------------------------------------------- menu parts

def sel_mark(sel):
    """Selection box: amber [x] when chosen, plain [ ] otherwise."""
    return c(A_AMBER, "[x]") if sel else "[ ]"


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
        return current
    if 1 <= idx <= len(options):
        return options[idx - 1]
    print("  out of range")
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


def notice(lines):
    """Amber message the user must actually get to see.

    The main loop clears the screen and re-renders the menu the moment a
    submenu returns, so anything printed after the last read is wiped in the
    same instant. Print the lines, then hold the screen until enter."""
    for text in (lines if isinstance(lines, (list, tuple)) else [lines]):
        print(c(A_AMBER, "  " + text))
    read_esc("  press enter to go back: ")


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


def menu_line(idx, label, value="", grey=True):
    plain = " %2d  %s" % (idx, label)
    left = c(A_GREY, plain) if grey else plain
    maxv = WIDTH - len(plain) - 3
    if maxv < 1:
        maxv = 1
    if len(value) > maxv:
        value = ("..." + value[-(maxv - 3):]) if maxv > 3 else value[:maxv]
    pad = WIDTH - len(plain) - len(value)
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
    if len(out) <= maxv:
        return out
    return out[:maxv]


def sub_lang_lines(width=WIDTH):
    """The subtitle code reference for menu item 7: space separated codes,
    filled to the box width with the remainder on the next line. Pure
    function, so the selftest can pin the layout without a tty."""
    lines = []
    row = ""
    for code in SUB_LANG_CODES.split():
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
    """The menu rows as (number, label, value, grey), numbered 1 to 12."""
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
        (11, "update yt-dlp cli", "", True),
        (12, "quit", "", False),
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
    while True:
        if dirty:
            if USE_COLOR:
                sys.stdout.write(CLEAR)
                sys.stdout.flush()
            render(s)
            hint(s)
            dirty = False
            results = False
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
            for line in sub_lang_lines():
                print(c(A_DARKGREY, line))
            raw = read_esc("  enter languages separated by commas (enter = keep): ")
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
                    said.append("not in the youtube list, kept: "
                                + ", ".join(unknown))
                if bad and not kept:
                    said.append("nothing valid left, kept "
                                + str(s["subtitle_langs"]))
                if said:
                    notice(said)
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
                    print("  not a number - kept %s" % s.get("playlist_max_items"))
    elif choice == "9":
        return multi_url(s)
    elif choice == "10":
        for k, v in DEFAULTS.items():
            s[k] = json.loads(json.dumps(v)) if isinstance(v, (dict, list)) else v
        print("  defaults restored")
    elif choice == "11":
        update_yt_dlp(s)
        read_esc("  press enter to go back: ")
    elif choice in ("12", "q", "quit", "exit"):
        save_settings(s)
        print("bye. defaults saved to %s" % CONFIG_PATH)
        raise SystemExit(0)
    elif choice:
        print("  pick 1-12 or paste a url")
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
        print("%-4s %s%s" % (ok, name, "" if cond else "  got: %s" % got))
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

    r429 = sub_fail_reason("ERROR: Unable to download video subtitles "
                           "for 'en': HTTP Error 429: Too Many Requests")
    check("sub reason 429", r429 == "the subtitle api answered 429 too many requests", r429)
    check("sub reason unknown -> none", sub_fail_reason("SOMETHING ODD") is None,
          sub_fail_reason("SOMETHING ODD"))

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
    check("progress size mint", A_MINT + "198.15MiB" + A_RESET in cp, cp)
    check("progress speed lblue", A_LBLUE + "2.32MiB/s" + A_RESET in cp, cp)
    check("progress eta purple", A_PURPLE + "01:12" + A_RESET in cp, cp)
    check("progress words grey", A_GREY + "of" + A_RESET in cp
          and A_GREY + "at" + A_RESET in cp
          and A_GREY + "ETA" + A_RESET in cp, cp)
    check("progress final line",
          A_AMBER + "100%" + A_RESET
          in color_progress("100% of 198.15MiB in 00:45"))

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
    check("menu has 12 rows", len(rows) == 12 and nums == list(range(1, 13)), nums)
    check("menu row 9 is multi url",
          rows[8][1] == "add multiple youtube urls", rows[8])
    check("menu row 12 is quit", rows[11][1] == "quit", rows[11])
    check("menu row 9 has no value",
          rows[8][2] == "" and rows[8][3] is True, rows[8])
    check("menu rows 10 and 11 shifted",
          rows[9][1] == "reset defaults"
          and rows[10][1] == "update yt-dlp cli", (rows[9], rows[10]))

    langs = sub_lang_lines()
    check("lang lines fit the box",
          langs and all(len(l) <= WIDTH for l in langs), langs)
    check("lang lines are indented and spaced",
          all(l.startswith("  ") and "  " not in l.strip() and l == l.rstrip()
              for l in langs), langs)
    flat = [w for l in langs for w in l.split()]
    check("lang line carries the groups",
          "en" in flat and "lt" in flat and "et" in flat and "ru" in flat
          and "zh" in flat and "vi" in flat, langs)
    check("lang codes are two letters",
          all(w.isalpha() and len(w) == 2 and w.islower() for w in flat), flat)
    check("lang first line fills the box", len(langs[0]) == WIDTH,
          len(langs[0]))
    check("lang rest on one second line",
          len(langs) == 2
          and [w for l in langs for w in l.split()] == SUB_LANG_CODES.split(),
          langs)
    check("lang lines wrap narrow", all(len(l) <= 24 for l in sub_lang_lines(24)),
          sub_lang_lines(24))

    check("lang ref covers every shown code",
          all(w in YOUTUBE_LANG_CODES for w in SUB_LANG_CODES.split()),
          [w for w in SUB_LANG_CODES.split() if w not in YOUTUBE_LANG_CODES])
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
    k6, b6, u6 = parse_lang_input("8")
    check("only digits leaves nothing", not k6 and b6 == ["8"] and not u6,
          (k6, b6, u6))

    # the menu repaints the moment a submenu returns, so a notice has to hold
    paused = []
    g = globals()
    real_esc = g["read_esc"]
    real_color = g["USE_COLOR"]
    g["read_esc"] = lambda prompt: paused.append(prompt)
    g["USE_COLOR"] = True
    cap3 = io.StringIO()
    old3 = sys.stdout
    sys.stdout = cap3
    try:
        notice(["one line", "second line"])
        notice("solo")
    finally:
        sys.stdout = old3
        g["read_esc"] = real_esc
        g["USE_COLOR"] = real_color
    body = cap3.getvalue()
    check("notice holds the screen once per call",
          len(paused) == 2 and "press enter" in paused[0], paused)
    check("notice prints every line amber",
          body.count(A_AMBER) == 3 and "one line" in body
          and "second line" in body and "solo" in body, repr(body))

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
        r13 = act("13", d)
        abody = cap4.getvalue()
    finally:
        sys.stdout = real_out
    check("hint points at item 9", "menu item: 9" in hbody, hbody)
    check("act rejects 13 with new range",
          "1-12" in abody and r13 is True, (abody, r13))

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
        return 0 if update_yt_dlp(s) else 1
    if args.url and not args.menu:
        return do_download(s, args.url)
    return main_menu(s)


if __name__ == "__main__":
    sys.exit(main())
