#!/usr/bin/env python3
"""Interactive CLI and quick launcher around yt-dlp.

Menu mode:   python src/ytdlp.py
Quick mode:  python src/ytdlp.py <url> [-a] [-f mp4] [-r 1080] [--subs]
Checks:      python src/ytdlp.py --check | --selftest

Stdlib only. Windows / Linux / macOS. Defaults persist in config/settings.json.
"""
import argparse
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import zipfile
from pathlib import Path

VERSION = "1.0"
ROOT = Path(__file__).resolve().parent.parent
CONFIG_PATH = ROOT / "config" / "settings.json"
FILES_DIR = ROOT / "ytdlp-files"
BIN_DIR = ROOT / "bin"
WIDTH = 64

A_AMBER = "\033[38;5;214m"
A_LBLUE = "\033[38;5;117m"
A_GREY = "\033[38;5;250m"
A_DARKGREY = "\033[38;5;244m"
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


class GoBack(Exception):
    """Escape pressed inside a submenu: return to the main menu."""


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
        buf = []
        while True:
            ch = msvcrt.getwch()
            if ch == "\r" or ch == "\n":
                sys.stdout.write("\n")
                sys.stdout.flush()
                return "".join(buf)
            if ch == "\x1b":
                sys.stdout.write("\r\033[K")
                sys.stdout.flush()
                return None
            if ch in ("\x08", "\x7f"):
                if buf:
                    buf.pop()
                    sys.stdout.write("\b \b")
                    sys.stdout.flush()
                continue
            if ch == "\x00":
                msvcrt.getwch()
                continue
            if ch == "\x03":
                raise KeyboardInterrupt
            buf.append(ch)
            sys.stdout.write(ch)
            sys.stdout.flush()
    import termios
    import tty
    fd = sys.stdin.fileno()
    old = termios.tcgetattr(fd)
    try:
        tty.setraw(fd)
        buf = []
        while True:
            ch = sys.stdin.read(1)
            if ch in ("\r", "\n"):
                sys.stdout.write("\n")
                sys.stdout.flush()
                return "".join(buf)
            if ch == "\x1b":
                sys.stdout.write("\r\033[K")
                sys.stdout.flush()
                return None
            if ch in ("\x7f", "\x08"):
                if buf:
                    buf.pop()
                    sys.stdout.write("\b \b")
                    sys.stdout.flush()
                continue
            if ch == "\x03":
                raise KeyboardInterrupt
            if ch >= " ":
                buf.append(ch)
                sys.stdout.write(ch)
                sys.stdout.flush()
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
    "output_dir": "downloads",
    "filename_template": "%(title)s",
    "subtitles": False,
    "subtitle_langs": "en,nl",
    "thumbnail": False,
    "embed_metadata": False,
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
PLATFORM_ZIP = {"windows": "yt-dlp-win64.zip", "linux": "yt-dlp_linux.zip"}
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
    CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
    tmp = CONFIG_PATH.with_suffix(".tmp")
    tmp.write_text(json.dumps(s, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    tmp.replace(CONFIG_PATH)


def onoff(flag):
    return "on" if flag else "off"


def resolve_outdir(s):
    out = s["output_dir"] or "downloads"
    p = Path(out)
    return p if p.is_absolute() else (ROOT / p)


# ---------------------------------------------------------------- binary lookup

def extract_yt_dlp():
    plat = platform_name()
    zname = PLATFORM_ZIP.get(plat)
    if zname:
        zpath = FILES_DIR / zname
        if zpath.exists():
            print("first run: unpacking %s -> bin/" % zname)
            BIN_DIR.mkdir(parents=True, exist_ok=True)
            with zipfile.ZipFile(zpath) as zf:
                zf.extractall(BIN_DIR)
            if plat != "windows":
                for p in BIN_DIR.rglob("*"):
                    if p.is_file():
                        p.chmod(p.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
            return BIN_DIR / PLATFORM_BIN.get(plat, "yt-dlp")
    return None


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
    got = extract_yt_dlp()
    if got and got.is_file():
        return got
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
    for c in (BIN_DIR / "ffmpeg", FILES_DIR / "ffmpeg"):
        if c.is_dir():
            return c
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


def build_args(s, url, ffdir):
    """Build yt-dlp argv (binary first, url last). ffdir is explicit: Path or
    None for "no ffmpeg" (forces single-file / no-convert branches)."""
    opts = []
    template = s["filename_template"] or DEFAULTS["filename_template"]
    opts += ["-o", template]
    if not ABS_RE.match(template):
        outdir = s["output_dir"] or "downloads"
        op = Path(outdir)
        opts += ["-P", "home:" + str(op if op.is_absolute() else resolve_outdir(s))]
    opts += ["--retries", "5"]

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

    if s.get("subtitles"):
        opts += ["--write-subs", "--write-auto-subs",
                 "--sub-langs", s["subtitle_langs"]]
        if ffdir:
            opts += ["--convert-subs", "srt"]

    if s.get("thumbnail"):
        if ffdir:
            opts += ["--embed-thumbnail"]
        else:
            opts += ["--write-thumbnail"]

    if s.get("embed_metadata") and ffdir:
        opts += ["--embed-metadata"]

    if not sys.stdout.isatty():
        opts += ["--newline"]

    return opts + [url]


# ---------------------------------------------------------------- download

def do_download(s, url):
    if not (url or "").lower().startswith(("http://", "https://")):
        print("not a URL: %r" % url)
        return 2
    binary = find_yt_dlp(s)
    if binary is None:
        plat = platform_name()
        print("yt-dlp binary not found.")
        if plat == "macos":
            print("no macOS zip shipped - install yt-dlp (brew install yt-dlp) or set yt_dlp_path in ")
            print("config/settings.json")
        else:
            print("expected a release zip in ytdlp-files/ or yt-dlp on PATH")
        return 2
    ver, rc = probe_version(binary)
    if rc != 0 or not ver:
        print("yt-dlp binary check failed (rc=%s): %s" % (rc, ver))
        return 2

    ffdir = find_ffmpeg_dir(s)
    if ffdir is None:
        kind = s.get("type", "video")
        if kind == "audio only" and s["audio_format"] not in ("best",):
            print("note: ffmpeg not found - saving original audio (no %s conversion)"
                  % s["audio_format"])
        elif kind == "video":
            print("note: ffmpeg not found - single best file, no merge")

    opts = build_args(s, url, ffdir)
    argv = [str(binary)] + opts
    print("-" * WIDTH)
    print("binary  : %s (%s)" % (binary, ver))
    print("url     : %s" % url)
    print("output  : %s" % resolve_outdir(s))
    print("command : %s" % " ".join(opts))
    print("-" * WIDTH)
    try:
        rc = subprocess.call(argv)
    except KeyboardInterrupt:
        print("\ncancelled by user")
        return 130
    if rc == 0:
        print("-" * WIDTH)
        print("finished - files in %s" % resolve_outdir(s))
        return 0
    print("yt-dlp exited with code %s" % rc)
    return rc


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


def hint(s):
    """Amber instructions plus grey detail lines, above the input field."""
    div = "-" * (WIDTH + 2)
    l1 = "enter number to change default settings"
    l2 = "paste url, press enter and download will start."
    l3 = "change download location by choosing menu item: 5"
    l4 = "DL location: %s" % resolve_outdir(s)
    print(c(A_AMBER, l1))
    print(c(A_AMBER, l2))
    print(c(A_DARKGREY, l3))
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


def render(s):
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
        (1, "change video/audio", shown, True),
        (2, "change container", container, True),
        (3, "change resolution", media, True),
        (4, "change audio format", s["audio_format"], True),
        (5, "change output folder",
         short_path(s["output_dir"] or "downloads",
                    WIDTH - len("  5  change output folder") - 3), True),
        (6, "change filename template", s["filename_template"], True),
        (7, "change subtitles", "%s (%s)" % (onoff(s.get("subtitles")), s["subtitle_langs"]), True),
        (8, "change thumbnail", onoff(s.get("thumbnail")), True),
        (9, "change metadata", onoff(s.get("embed_metadata")), True),
        (10, "change playlist", pl, True),
        (11, "reset defaults", "config/settings.json", True),
        (12, "quit", "", False),
    ]
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
    while True:
        if dirty:
            if USE_COLOR:
                sys.stdout.write(CLEAR)
                sys.stdout.flush()
            render(s)
            hint(s)
            dirty = False
        try:
            choice = read_esc("> ")
        except (EOFError, KeyboardInterrupt):
            print()
            return 0
        if choice is None:
            # escape at the main prompt: stay put, do not re-render
            continue
        choice = choice.strip()
        url = clean_url(choice)
        if url:
            do_download(s, url)
            dirty = True
            save_settings(s)
            continue
        try:
            act(choice, s)
        except GoBack:
            pass
        dirty = True
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


def act(choice, s):
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
            raw = read_esc("  languages (comma list, enter = keep): ")
            if raw is None:
                raise GoBack()
            raw = raw.strip()
            if raw:
                s["subtitle_langs"] = raw
    elif choice == "8":
        s["thumbnail"] = ask_bool("thumbnail", s.get("thumbnail"))
    elif choice == "9":
        s["embed_metadata"] = ask_bool("embed metadata", s.get("embed_metadata"))
    elif choice == "10":
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
    elif choice == "11":
        for k, v in DEFAULTS.items():
            s[k] = json.loads(json.dumps(v)) if isinstance(v, (dict, list)) else v
        print("  defaults restored")
    elif choice in ("12", "q", "quit", "exit"):
        save_settings(s)
        print("bye. defaults saved to %s" % CONFIG_PATH)
        raise SystemExit(0)
    elif choice:
        print("  pick 1-12 or paste a url")


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
    check("subs + langs", "--sub-langs" in a and a[a.index("--sub-langs") + 1] == "en,nl", a)
    check("subs convert guarded by ffmpeg", "--convert-subs" in a, a)

    tmp["subtitles"] = False
    a = build_args(tmp, "https://x/y", None)
    check("no convert-subs without ffmpeg", "--convert-subs" not in a, a)

    tmp["thumbnail"] = True
    a = build_args(tmp, "https://x/y", None)
    check("write thumbnail without ffmpeg", "--write-thumbnail" in a and "--embed-thumbnail" not in a, a)

    tmp["filename_template"] = "C:/out/%(title)s.%(ext)s"
    a = build_args(tmp, "https://x/y", fd)
    check("absolute template -> no -P", "-P" not in a, a)

    tmp["filename_template"] = "play/%(title)s.%(ext)s"
    a = build_args(tmp, "https://x/y", fd)
    check("relative template keeps -P", a[a.index("-P") + 1].startswith("home:"), a)

    check("clean_url basic", clean_url("watch https://youtu.be/abc-123_x now") == "https://youtu.be/abc-123_x", clean_url("watch https://youtu.be/abc-123_x now"))
    check("clean_url trims punctuation", clean_url("https://youtu.be/z9?si=1, ok") == "https://youtu.be/z9?si=1", clean_url("https://youtu.be/z9?si=1, ok"))
    check("clean_url none", clean_url("no link here") is None, clean_url("no link here"))

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
    p.add_argument("--selftest", action="store_true", help="run built-in logic tests")
    args = p.parse_args(argv)

    s = load_settings()
    apply_overrides(s, args)

    if args.check:
        show_check(s)
        return 0
    if args.selftest:
        return selftest()
    if args.url and not args.menu:
        return do_download(s, args.url)
    return main_menu(s)


if __name__ == "__main__":
    sys.exit(main())
