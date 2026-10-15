## 6. Functionality

Startup journey: enable ANSI/VT where needed (L47-L59), paint menu box (header "yt-dlp cli  v1.5.0" centered, blank row, 13 rows, "press escape to exit submenu", rules, L1872-L1945), print the hint block above the input line - verbatim, amber then grey then divider (hint L1856-L1869):

    paste url, press enter and download will start.
    enter number to change default settings
    change download location by choosing menu item: 5
    download several urls at once by choosing menu item: 9
    DL location: <resolved folder>

then auto-update check (FR-13) and first save (L1968-L1972). The menu never disappears: everything paints under it, nothing prints beyond the hint block, item-9 entry lines, and stage/verdict lines (main_menu loop L1954-L2005).

Primary flow (single url): prompt accepts text; first http(s) substring is the url (before any number is consulted, L1984-L1997) - when no url is present, text goes to act(); do_download per FR-7: preflight freeze, then the spinner stage with live progress, then exactly one of three justified verdicts: finished (plus "... <name> (<MB>)" file line via newest_since, 2 s slack, L1146-L1160), failed (full error line), or stopped-by-escape (partial cleanup count). States covered: not a url (message), yt-dlp missing (guidance), yt-dlp fails its check (frozen rc), no ffmpeg (offer per FR-14 or declined continuation), success, failure (always ends in a readable frozen line), escape (kill + clean). Stage lines persist until the next url, menu pick, repaint or escape (L1954-L2005 management of results/dirty).

Batch flow (item 9): FR-12 above; states: empty entry closes and starts, escape returns without downloads, bad line marked and skipped, per-url header line, batch aborted by escape only.

Updaters (items 11/12): FR-13; states: current version ("already current"), newer version offered with enter = no, network down (silent or, when forced, one explanatory line), older published file (no version found message when forced).

Settings flows (items) each end in: new value shown, a hold read, menu repaint; item 10 restores DEFAULTS and answers "defaults restored" (L2329-L2333); item 13 saves and prints the settings path (L2344-L2347).

Failure handling pattern: every detectable failure (missing binary, failed probe, refused install, checksum mismatch, unreadable archive, network loss) ends in a specific user-visible line describing the fix or the kept state; nothing fails silently except non-forced network contact (NFR, L2174-L2179).

