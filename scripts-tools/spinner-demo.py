"""Terminal demo of five 4-char waiting animations for the download flow.

Standalone: stdlib only, no import from the project, runs from any folder.
In a real console it needs no arguments, piped it prints the frames as text.

Run in a real console:  python scripts-tools/spinner-demo.py     (all five)
One animation only:    python scripts-tools/spinner-demo.py 3
Piped (no tty):        prints the frames as text and exits.

Each entry is (description, ansi 256-color, frames) and every frame is
exactly four characters wide, matching the menu row budget of 66 chars.
"""
import os
import sys
import time

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
    """Windows legacy consoles need ENABLE_VIRTUAL_TERMINAL_PROCESSING for
    ANSI. Same guard the CLI uses, inlined so this demo is a standalone file:
    it imports nothing from the project."""
    if os.name != "nt":
        return
    import ctypes
    k = ctypes.windll.kernel32
    h = k.GetStdHandle(-11)
    mode = ctypes.c_uint32()
    if k.GetConsoleMode(h, ctypes.byref(mode)):
        k.SetConsoleMode(h, mode.value | 0x0004)


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
