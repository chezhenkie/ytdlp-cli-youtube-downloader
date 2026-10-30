third-party notices
===================

This bundle contains four independently licensed projects. They are separate
programs that work together because one invokes the other; each keeps its own
license.

Which of the four are inside a given artifact: the platform bundles carry
yt-dlp and srtstripper (the portable ones also ffmpeg), the macOS pkg carries
yt-dlp, ffmpeg and srtstripper, and the linux-musl bundle carries ytdlp-cli and
the musl build of srtstripper only - there yt-dlp and ffmpeg come from the
system package manager (on Alpine, apk add yt-dlp pulls ffmpeg with it).

1. ytdlp-cli - the menu and the defaults
   MIT License. See LICENSE in this bundle.

2. yt-dlp - performs every download
   The Unlicense, a public domain dedication.
   https://github.com/yt-dlp/yt-dlp
   The bundled yt-dlp ships its own THIRD_PARTY_LICENSES.txt listing every
   library inside it. That file is part of yt-dlp's own distribution and is not
   reproduced here. In the linux-musl bundle yt-dlp is not bundled at all; the
   system's own copy and its license file apply.

3. ffmpeg - merging, audio extraction and subtitle conversion
   Present in the portable bundles and the macOS pkg; the lean and linux-musl bundles do not contain it.
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

   - Linux build, ffmpeg n9.0.2-39-g7adfbe40dc-20261030, provided by
     BtbN/FFmpeg-Builds.
     Archive: ffmpeg-n9.0.2-39-g7adfbe40dc-linux64-gpl-9.0.tar.xz (autobuild-2026-10-30-13-01)
     sha256: 2f787799c77ac8eda96f74fee82e7b18ef8f7c8c57ff99da68cb45046937092c
     Release page: https://github.com/BtbN/FFmpeg-Builds/releases
     The scripts used to build these binaries live in that same repository.

   - macOS build (arm64), ffmpeg n8.1.2, provided by
     shaka-project/static-ffmpeg-binaries.
     Asset: ffmpeg-osx-arm64 (release n8.1.2-1)
     sha256: e7b9fcd97f95f333512d6e8b8ac24d9dbc08f189f36047695499bd7b57214b22
     Release page: https://github.com/shaka-project/static-ffmpeg-binaries/releases
     The build script and the CI workflow that produce these binaries live in
     that same repository. Present in the macOS pkg.

   ffmpeg is free software. You may use, study, modify and redistribute it under
   the terms of the GNU General Public License, exactly as its authors intend.
   Nothing in this bundle, and nothing in ytdlp-cli's own license or
   documentation, adds to, limits or restricts the rights the GPL grants you
   for ffmpeg.

4. srtstripper - writes the timecode-stripped copy next to a downloaded
   subtitle (menu item 8)
   MIT License, same author as ytdlp-cli (chezhenkie).
   https://github.com/chezhenkie/srt-subtitle-stripper
   Released as v1.6.4; the pinned builds shipped in bin/:
   - Windows: srtstripper-windows-x86_64.exe,
     sha256 8cd030d4db776e3db1ca16bb286c1eeaecc71dd99c4ddb162331b0d68cb14cbb
   - Linux (glibc): srtstripper-linux-x86_64,
     sha256 85196f96349cdc0ea6c2bb54b8aee9bc5fc2d28d8ad2a51df00453c8cdac2fd1
   - Linux (musl, static): srtstripper-linux-x86_64-musl,
     sha256 0a1a7ae08b4b54fc97da2313fe78b0c02185c4110994e9e45e3d3f60498fef9b
   - macOS (arm64): srtstripper-macos-arm64,
     sha256 4d70f0049be33ca63faf0cad6bc0c2a73ea29ff8bc1fd8f52c3cffea6566913c
   The linux-musl bundle ships the musl build; the glibc and macOS builds are
   not present there.
