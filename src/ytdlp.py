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

URL_RE = re.compile(r"https?://\S+")
ABS_RE = re.compile(r"^([A-Za-z]:[\\/]|[\\/])")

DEFAULTS = {
    "settings_version": 1,
    "yt_dlp_path": "",
    "ffmpeg_path": "",
    "video_container": "mp4",
    "video_resolution": "1080",
    "audio_only": False,
    "audio_format": "mp3",
    "audio_quality": "0",
    "output_dir": "downloads",
    "filename_template": "%(title)s [%(id)s].%(ext)s",
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
    hit = shutil.which("ffmpeg")
    if hit:
        return Path(hit).parent
    for c in (BIN_DIR / "ffmpeg", FILES_DIR / "ffmpeg"):
        if c.is_dir():
            return c
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

    if s.get("audio_only"):
        if ffdir:
            opts += ["-f", "bestaudio/best",
                     "-x",
                     "--audio-format", s["audio_format"],
                     "--audio-quality", str(s["audio_quality"])]
        else:
            opts += ["-f", "bestaudio"]
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
        if s.get("audio_only") and s["audio_format"] not in ("best",):
            print("note: ffmpeg not found - saving original audio (no %s conversion)"
                  % s["audio_format"])
        elif not s.get("audio_only"):
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

def pick_list(title, options, current):
    print("  %s" % title)
    for i, o in enumerate(options, 1):
        mark = "x" if str(o) == str(current) else " "
        print("    [%s] %2d  %s" % (mark, i, o))
    raw = input("  number (Enter = keep %s): " % current).strip()
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
    raw = input("  %s (now %s) y/n (Enter = keep): " % (label, onoff(current))).strip().lower()
    if raw in ("y", "yes", "on", "1"):
        return True
    if raw in ("n", "no", "off", "0"):
        return False
    return current


def menu_line(idx, label, value=""):
    left = " %2d  %s" % (idx, label)
    right = value
    pad = WIDTH - len(left) - len(right) - 2
    if pad < 1:
        right = ""
        pad = WIDTH - len(left) - 2
    return "|" + left + (" " * pad) + right + "|"


def render(s):
    bar = "+" + "-" * WIDTH + "+"
    kind = "audio only (%s)" % s["audio_format"] if s.get("audio_only") else "video"
    res = s["video_resolution"]
    if s.get("audio_only"):
        media = "container -"
    else:
        media = "%sp" % res if res != "best" else "best"
    rows = [
        ("", ""),
        menu_line(1, "Paste URL and download", "uses the defaults below"),
        ("", ""),
        menu_line(2, "Type", kind),
        menu_line(3, "Container", s["video_container"] if not s.get("audio_only") else "-"),
        menu_line(4, "Resolution", media),
        menu_line(5, "Audio format", s["audio_format"]),
        menu_line(6, "Output folder", str(resolve_outdir(s))),
        menu_line(7, "Filename template", s["filename_template"][:20]),
        menu_line(8, "Subtitles", "%s (%s)" % (onoff(s.get("subtitles")), s["subtitle_langs"])),
        menu_line(9, "Thumbnail", onoff(s.get("thumbnail"))),
        menu_line(10, "Embed metadata", onoff(s.get("embed_metadata"))),
        menu_line(11, "Playlist", onoff(s.get("playlist"))),
        menu_line(12, "Reset defaults", "config/settings.json"),
        menu_line(13, "Quit", ""),
        ("", ""),
    ]
    print(bar)
    print("|" + "yt-dlp CLI  v%s".ljust(WIDTH) + "|" % VERSION)
    print(bar)
    for r in rows:
        print(r if r else "|" + " " * WIDTH + "|")
    print(bar)


def menu_summary(s):
    if s.get("audio_only"):
        return "audio-only %s" % s["audio_format"]
    return "%s %sp" % (s["video_container"], s["video_resolution"])


def main_menu(s):
    while True:
        render(s)
        print("changes save automatically - paste-URL flow is option 1")
        try:
            choice = input(" select > ").strip()
        except (EOFError, KeyboardInterrupt):
            print()
            return 0
        if choice == "1":
            raw = input(" URL (Enter = cancel): ").strip()
            url = clean_url(raw)
            if not url:
                print(" no URL found in that text")
                continue
            do_download(s, url)
            input(" Enter to continue...")
        elif choice == "2":
            s["audio_only"] = not s.get("audio_only")
        elif choice == "3":
            if s.get("audio_only"):
                print("  audio-only mode has no container")
                continue
            s["video_container"] = pick_list("Container", CONTAINERS, s["video_container"])
        elif choice == "4":
            if s.get("audio_only"):
                print("  audio-only mode has no resolution")
                continue
            s["video_resolution"] = pick_list("Max height", RESOLUTIONS, s["video_resolution"])
        elif choice == "5":
            s["audio_format"] = pick_list("Audio format (needs ffmpeg to convert)", AUDIO_FORMATS, s["audio_format"])
        elif choice == "6":
            raw = input("  folder (Enter = keep %s): " % resolve_outdir(s)).strip().strip('"')
            if raw:
                s["output_dir"] = raw
        elif choice == "7":
            raw = input("  template (Enter = keep): ").strip()
            if raw:
                s["filename_template"] = raw
        elif choice == "8":
            s["subtitles"] = ask_bool("Subtitles", s.get("subtitles"))
            if s.get("subtitles"):
                raw = input("  languages (comma list, Enter = keep): ").strip()
                if raw:
                    s["subtitle_langs"] = raw
        elif choice == "9":
            s["thumbnail"] = ask_bool("Thumbnail", s.get("thumbnail"))
        elif choice == "10":
            s["embed_metadata"] = ask_bool("Embed metadata", s.get("embed_metadata"))
        elif choice == "11":
            s["playlist"] = ask_bool("Playlist mode", s.get("playlist"))
            if s.get("playlist"):
                raw = input("  max items (0 = all, Enter = keep %s): "
                            % s.get("playlist_max_items")).strip()
                if raw:
                    try:
                        s["playlist_max_items"] = int(raw)
                    except ValueError:
                        print("  not a number - kept %s" % s.get("playlist_max_items"))
        elif choice == "12":
            for k, v in DEFAULTS.items():
                s[k] = json.loads(json.dumps(v)) if isinstance(v, (dict, list)) else v
            print("  defaults restored")
        elif choice in ("13", "q", "quit", "exit"):
            save_settings(s)
            print("bye. defaults saved to %s" % CONFIG_PATH)
            return 0
        else:
            print("  pick 1-13")
        save_settings(s)


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

    tmp["audio_only"] = True
    a = build_args(tmp, "https://x/y", fd)
    check("audio extract", "-x" in a and a[a.index("--audio-format") + 1] == "mp3", a)
    check("audio quality", a[a.index("--audio-quality") + 1] == "0", a)

    a = build_args(tmp, "https://x/y", None)
    check("no ffmpeg -> bestaudio", a[a.index("-f") + 1] == "bestaudio", a)
    check("no ffmpeg -> no -x", "-x" not in a, a)

    tmp["audio_only"] = False
    a = build_args(tmp, "https://x/y", None)
    check("no ffmpeg video -> -f b", a[a.index("-f") + 1] == "b", a)
    check("no ffmpeg -> no ffmpeg-location", "--ffmpeg-location" not in a, a)

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
        s["audio_only"] = True
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
