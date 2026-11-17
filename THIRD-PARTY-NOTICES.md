third-party notices
===================

This bundle contains four independently licensed projects. They are separate
programs that work together because one invokes the other; each keeps its own
license.

Which of the four are inside a given artifact: the windows zip, the windows
setup.exe, the windows arm64 zip, the linux glibc tars and the macOS pkg carry
yt-dlp, ffmpeg and srtstripper; the linux-musl bundles carry ytdlp-cli and the
musl build of srtstripper only - there yt-dlp and ffmpeg come from the system
package manager (on Alpine, apk add yt-dlp pulls ffmpeg with it).

1. ytdlp-cli - the menu and the defaults
   MIT License. See LICENSE in this bundle.

2. yt-dlp - performs every download
   The Unlicense, a public domain dedication.
   https://github.com/yt-dlp/yt-dlp
   The bundled copy is the official onedir build - `bin/yt-dlp[.exe]` plus its
   `_internal` runtime folder - unpacked from the yt-dlp release zip unchanged;
   menu item 13 replaces the pair as a tree. It ships its own
   THIRD_PARTY_LICENSES.txt listing every library inside it. That file is part
   of yt-dlp's own distribution and is not reproduced here. In the linux-musl bundle yt-dlp is not bundled at all; the
   system's own copy and its license file apply.

3. ffmpeg - merging, audio extraction and subtitle conversion
   Present in every glibc bundle and in the macOS pkg; the linux-musl bundles
   do not contain it. The windows arm64 bundle carries the x64 build below:
   no arm64 windows build is published by the provider, and Windows 11 runs the
   x64 binary under emulation.
   Copyright (c) 2000-2026 the FFmpeg developers.
   The builds shipped here are configured --enable-gpl --enable-version3, so
   they are distributed under the GNU General Public License version 2 or later,
   and they include components licensed under GPL version 3 or later. The full
   text of GPL version 2 and of GPL version 3 both ship in this bundle, as
   GPL-2.0.txt and GPL-3.0.txt.

   Corresponding source. FFmpeg's own source is published at
   https://ffmpeg.org/download.html and in the git repository at
   https://github.com/FFmpeg/FFmpeg. All three bundled binaries come from one
   provider, shaka-project/static-ffmpeg-binaries release n8.1.2-1, built from
   the ffmpeg n8.1.2 tree with libvpx 1.16.0, svt-av1 4.1.0, x264, x265 4.2,
   lame 3.100, opus 1.6.1 and mbedtls 3.4.1. The build script and the CI
   workflow that produce these binaries live in that same repository:
   https://github.com/shaka-project/static-ffmpeg-binaries

   - Windows build: asset ffmpeg-win-x64.exe
     sha256: 4044b3924c977ad31229d504c5d5b8685f9553124fbaff6e9c99048b42830341
   - Linux build: asset ffmpeg-linux-x64
     sha256: 9eac5b2b5076db5ff853a6fa0dcd6b8de7d0cac8481eadda6c47cd935825f1ee
   - Linux build (arm64): asset ffmpeg-linux-arm64
     sha256: 6e7b1d7d1aa8c35e3fedd78a140aa0968717aeb7386ecfb0ee00773d9f0a4503
   - macOS build (arm64): asset ffmpeg-osx-arm64
     sha256: e7b9fcd97f95f333512d6e8b8ac24d9dbc08f189f36047695499bd7b57214b22

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
   - Windows (x64): srtstripper-windows-x86_64.exe,
     sha256 8cd030d4db776e3db1ca16bb286c1eeaecc71dd99c4ddb162331b0d68cb14cbb
   - Windows (arm64): srtstripper-windows-aarch64.exe,
     sha256 42fdad5259105ded2daa5ccff5c5d13144d939035f30a2e12f3b829e9499c849
   - Linux (glibc): srtstripper-linux-x86_64,
     sha256 85196f96349cdc0ea6c2bb54b8aee9bc5fc2d28d8ad2a51df00453c8cdac2fd1
   - Linux (glibc, arm64): srtstripper-linux-aarch64,
     sha256 25b2ca6e3e034d6363616e49811a93231124fa7c706c0b1fdecf185eb605a582
   - Linux (musl, static, x64): srtstripper-linux-x86_64-musl,
     sha256 0a1a7ae08b4b54fc97da2313fe78b0c02185c4110994e9e45e3d3f60498fef9b
   - Linux (musl, static, arm64): srtstripper-linux-aarch64-musl,
     sha256 9218bb785f8270092847d93d0d603a2a0eaff013b42db650cbea5eeb1229148b
   - macOS (arm64): srtstripper-macos-arm64,
     sha256 4d70f0049be33ca63faf0cad6bc0c2a73ea29ff8bc1fd8f52c3cffea6566913c
   The linux-musl bundles ship the musl build; the glibc and macOS builds are
   not present there.
