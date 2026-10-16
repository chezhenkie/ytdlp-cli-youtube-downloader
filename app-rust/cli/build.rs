//! FR-16: the Windows PE version resource. Explorer's Properties dialog
//! (Details tab) shows the product identity: version, owner, copyright
//! and the repository url. Build-time only and windows builds only; the
//! build script itself is compiled for the host, so every other platform
//! skips the block below.

fn main() {
    #[cfg(windows)]
    {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
        let mut res = winresource::WindowsResource::new();
        res.set(
            "FileDescription",
            "yt-dlp cli - text-menu download wrapper around yt-dlp",
        );
        res.set("ProductName", "yt-dlp cli");
        res.set("CompanyName", "chezhenkie");
        res.set("LegalCopyright", "MIT License");
        res.set("FileVersion", &format!("{version}.0"));
        res.set("ProductVersion", &version);
        res.set("InternalName", "ytdlp");
        res.set("OriginalFilename", "ytdlp.exe");
        res.set(
            "Comments",
            "https://github.com/chezhenkie/ytdlp-cli-youtube-downloader",
        );
        if let Err(e) = res.compile() {
            panic!("failed to embed the Windows version resource: {e}");
        }
    }
}
