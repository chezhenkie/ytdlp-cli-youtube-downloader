## 8. Data

settings.json fields; all strings unless stated, none sensitive (no secrets, no machine-derived values shipped), owner chezhenkie, retained indefinitely in the user's copy, migration via settings_version (currently unused, OQ-6). Boundary validation (audit F1/F2, 2026-10-14): bool fields are safe by truthiness; playlist_max_items, video_resolution, video_container, audio_format and audio_quality reach their consumers raw from the editable file. Fix design: load_settings coerces these in memory only - playlist_max_items int-parsed with fallback 0 (= all), the three vocabulary fields whitelisted against RESOLUTIONS/CONTAINERS/AUDIO_FORMATS with fallback to the default, audio_quality accepted only as a number or "<n>K" per yt-dlp --audio-quality SPEC semantics, fallback "0". Coercion never rewrites settings.json (user settings are theirs):

| key | type | shipped | allowed/default | consumed by |
|---|---|---|---|---|
| settings_version | int | 1 | schema marker | none yet (L320) |
| yt_dlp_path | str | "" | path or "" (auto) | find_yt_dlp L620-L622 |
| ffmpeg_path | str | "" | dir or file path or "" | find_ffmpeg_dir L633-L638 |
| auto_update | bool | true | on/off | auto_update_check L2164 |
| auto_update_last_check | int | 1791925032* | unix epoch seconds | throttle L2168-L2173 |
| type | str | "video" | video / video no audio / audio only | build_args L772 |
| video_container | str | "mp4" | mp4 mkv webm best | build_args L791-L792 |
| video_resolution | str | "1080" | 2160..360, best | build_args L782,702-705 |
| audio_format | str | "mp3" | mp3 m4a opus wav best | build_args L777 |
| audio_quality | str | "0" | ffmpeg audio -q value | build_args L778 |
| output_dir | str | "" | "" = resolve per FR-6 | resolve_outdir L583-L586 |
| filename_template | str | "%(title)s" | yt-dlp -o template | output_args L754 |
| subtitles | bool | true (default on, L333) | on/off | do_download L1623 |
| subtitle_langs | str | "en" | comma list, wildcards pass | build_sub_args L818 |
| playlist | bool | false | on/off | build_args L799-L804 |
| playlist_max_items | int | 0 | 0 = all | build_args L800-L802 |

*Only auto_update_last_check differs from the code default: DEFAULTS carries 0, the live settings.json carries the stamp from the last tested run - live state, committed on purpose (DEFAULTS L319-L337). Loader copies only known keys and ignores unknown json (L473-L475).

