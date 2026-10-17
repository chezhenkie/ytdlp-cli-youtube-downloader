# app-python is deprecated - frozen at v1.5.0

The native line in app-rust/ is the product. This folder is the final python release (v1.5.0, 2026-10-15) and receives no further development, fixes or releases.

It stays in the repo for one reason: it is the reference oracle the CI differential, check-parity and session jobs compare the native build against (CPython 3.14.4 exactly). Changing anything here would change the oracle and invalidate those jobs - do not edit it.

Do not run it from this folder. With a real tty it goes live: the startup auto-update check dials the network and stamps config/settings.json. Copy the source to a scratch tree, or drive it through the session_diff staged trees only.

- Product: app-rust/ (native, shipped as the ytdlp-cli releases)
- Legacy bundles: v1.5.0 on the release page
- Specsheet: the reference-oracle note in specsheet/00 is the single python mention
