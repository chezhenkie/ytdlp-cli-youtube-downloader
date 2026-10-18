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

1. ytdlp-cli - the menu, the launchers and the defaults
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

   - Linux build, ffmpeg n9.0.2-27-g7e93014c5b-20261013, provided by
     BtbN/FFmpeg-Builds.
     Archive: ffmpeg-n9.0.2-27-g7e93014c5b-linux64-gpl-9.0.tar.xz (autobuild tag)
     sha256: 50e0ede136e3545758a8dfb2e6cd12ec865643cd138af9ce6cd36a9015f395c2
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
   Released as v1.0.1; the pinned builds shipped in bin/:
   - Windows: srtstripper-windows-x86_64.exe,
     sha256 05230e669c4ec9badcb04c20cc81ca74f97c5d593898e82a22206799aacc07ca
   - Linux (glibc): srtstripper-linux-x86_64,
     sha256 c0b5b8066f6ac3af0eaf6bdcd3e7589f5b1e07787d3c79d9c5151b06413f585c
   - Linux (musl, static): srtstripper-linux-x86_64-musl,
     sha256 77def542f57c45cb8b02c92d2110a7e374fb9f767342cc71dd555ce8830be098
   - macOS (arm64): srtstripper-macos-arm64,
     sha256 2d5cc6e9a0baba2b5d3a656e3db4c00bdfb601fd545ef7e3f8f7f9748b772961
   The linux-musl bundle ships the musl build; the glibc and macOS builds are
   not present there.
