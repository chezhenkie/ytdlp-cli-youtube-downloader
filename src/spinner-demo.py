"""Terminal demo of five 4-char waiting animations for the download flow.

Run in a real console:  python src/spinner-demo.py          (all five)
One animation only:     python src/spinner-demo.py 3
Piped (no tty):         prints the frames as text and exits.

Each entry is (description, ansi 256-color, frames) and every frame is
exactly four characters wide, matching the menu row budget of 66 chars.
"""
import os
import sys
import time
from pathlib import Path

A_AMBER = "\033[38;5;214m"
A_LBLUE = "\033[38;5;117m"
A_GREY = "\033[38;5;250m"
A_DARKGREY = "\033[38;5;244m"
A_RESET = "\033[0m"

BUSY = 0.08  # seconds per frame

SPINNERS = [
    ("dots: a dot grows to full width, then shrinks back", A_AMBER,
     [".   ", "..  ", "... ", "....", " ...", "  ..", "   .", "  ..", " ..."]),
    ("bar: a hash fill sweeps out and back", A_LBLUE,
     ["#---", "##--", "###-", "####", "-###", "--##", "---#", "--##", "-###"]),
    ("spin: classic tick rotating in the middle column", A_GREY,
     [" /  ", " -  ", " \\  ", " |  "]),
    ("brackets: a paren runs out, closes on the way back", A_DARKGREY,
     ["(   ", " (  ", "  ( ", "   )", "  ) ", " )  "]),
    ("return: chevrons run out, then flow back in", A_LBLUE,
     [">   ", ">>  ", ">>> ", ">>>>", " >>>", "  >>", "   >", "  > ", " >  "]),
]


def play(frames, color, seconds=4.0):
    t0 = time.time()
    i = 0
    while time.time() - t0 < seconds:
        sys.stdout.write("\r%s%s%s" % (color, frames[i % len(frames)], A_RESET))
        sys.stdout.flush()
        i += 1
        time.sleep(BUSY)
    sys.stdout.write("\r    \r")
    sys.stdout.flush()


def _enable_vt():
    """Same VT enable the CLI does at menu start (windows legacy consoles)."""
    if os.name != "nt":
        return
    import importlib.util
    spec = importlib.util.spec_from_file_location(
        "ytdlp", str(Path(__file__).resolve().parent / "ytdlp.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    mod.enable_vt_output()


def main():
    idx = None
    if len(sys.argv) > 1:
        try:
            idx = int(sys.argv[1])
        except ValueError:
            print("usage: spinner-demo.py [1-5]")
            return 2
        if not 1 <= idx <= len(SPINNERS):
            print("usage: spinner-demo.py [1-%d]" % len(SPINNERS))
            return 2
    if not sys.stdout.isatty():
        for n, (name, _color, frames) in enumerate(SPINNERS, 1):
            print("%d  %s" % (n, name))
            print("    frames: |%s|" % "|".join(frames))
        print("run this in a real console to watch the animations")
        return 0
    _enable_vt()
    picks = [idx] if idx else list(range(1, len(SPINNERS) + 1))
    for n in picks:
        name, color, frames = SPINNERS[n - 1]
        print("%d  %s" % (n, name))
        play(frames, color)
    print("reply with the number you want, or none")
    return 0


if __name__ == "__main__":
    sys.exit(main())
