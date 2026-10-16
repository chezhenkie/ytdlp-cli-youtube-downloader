third-party notices
===================

This bundle contains three independently licensed projects. They are separate
programs that work together because one invokes the other; each keeps its own
license.

1. ytdlp-cli - the menu, the launchers and the defaults
   MIT License. See LICENSE in this bundle.

2. yt-dlp - performs every download
   The Unlicense, a public domain dedication.
   https://github.com/yt-dlp/yt-dlp
   The bundled yt-dlp ships its own THIRD_PARTY_LICENSES.txt listing every
   library inside it. That file is part of yt-dlp's own distribution and is not
   reproduced here.

3. ffmpeg - merging, audio extraction and subtitle conversion
   Present in the portable bundles only; the lean bundles do not contain it.
   Copyright (c) 2000-2026 the FFmpeg developers.
   The builds shipped here are configured --enable-gpl --enable-version3, so
   they are distributed under the GNU General Public License version 2 or later,
   and they include components licensed under GPL version 3 or later. The full
   text of GPL version 2 and of GPL version 3 both ship in this bundle, as
   GPL-2.0.txt and GPL-3.0.txt.

   Corresponding source. FFmpeg's own source is published at
   https://ffmpeg.org/download.html and in the git repository at
   https://github.com/FFmpeg/FFmpeg. The exact tree each bundled binary was
   built from is named by the provider:

   - Windows build, ffmpeg 9.0.2 essentials build, provided by gyan.dev.
     Archive: ffmpeg-9.0.2-essentials_build.zip
     sha256: 60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba
     Build page: https://www.gyan.dev/ffmpeg/builds/
     That page links the corresponding source of each build it publishes,
     marked "source code @ github". gyan.dev publishes Windows binaries only,
     so the Linux build cannot come from there.

   - Linux build, ffmpeg n9.0.2-27-g7e93014c5b-20261013, provided by
     BtbN/FFmpeg-Builds.
     Archive: ffmpeg-n9.0.2-27-g7e93014c5b-linux64-gpl-9.0.tar.xz (autobuild tag)
     sha256: 50e0ede136e3545758a8dfb2e6cd12ec865643cd138af9ce6cd36a9015f395c2
     Release page: https://github.com/BtbN/FFmpeg-Builds/releases
     The scripts used to build these binaries live in that same repository.

   ffmpeg is free software. You may use, study, modify and redistribute it under
   the terms of the GNU General Public License, exactly as its authors intend.
   Nothing in this bundle, and nothing in ytdlp-cli's own license or
   documentation, adds to, limits or restricts the rights the GPL grants you
   for ffmpeg.
