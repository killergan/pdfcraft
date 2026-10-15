# The BEM fork

What this fork changes on top of upstream `storytold/pdfcraft`, and how its builds update themselves.
Keep this list short: every line is something a merge from upstream can conflict with.

## Changes

| What | Where |
|---|---|
| No Discord button in the top bar, no community card on the home screen. The Help menu and About dialog keep the community links | `crates/ui-egui/src/chrome.rs`, `crates/ui-egui/src/home.rs` |
| App icon: PDF-red tile, white capital A, small BEM under it. The Windows PDF file icon has a red badge | `assets/app-icon/*`, `ATTRIBUTION.toml` |
| Documents open as just the page: Tools panel and side pane closed, whatever was open before (`PdfCraftApp::close_panels_on_open`, turned on by the desktop app) | `crates/ui-egui/src/lib.rs` (`finish_open`), `apps/pdfcraft/src/main.rs` |
| Silent self-update from this fork's releases (Windows) | `apps/pdfcraft/src/autoupdate.rs`, `.github/workflows/bem-release.yml` |

## How updating works

1. `bem-release.yml` runs daily (and on demand from the Actions tab). It merges upstream `main` into
   this fork's `main`, builds `pdfcraft.exe` with `PDFCRAFT_BUILD_LABEL=bem-<run number>` and
   publishes release `bem-<run number>` holding `pdfcraft-windows-x86_64.exe` and its `.sha256`.
   Nothing is built when neither side moved since the last release. A merge conflict fails the run
   (GitHub emails you); resolve it on a branch and merge that into `main`.
2. A labelled build checks the fork's latest release about 20 seconds after start. If the release
   number is higher it downloads the exe, checks the SHA-256, renames the running exe to
   `pdfcraft.exe.old` and puts the new one in its place. It runs from the next launch. No dialog.
3. A build without the `bem-<N>` label (plain `cargo build`) never updates itself.
   `PDFCRAFT_NO_AUTO_UPDATE=1` turns updating off for a labelled build.

First install: download `pdfcraft-windows-x86_64.exe` from the fork's releases and keep it in a folder
you can write to (not `Program Files`: the swap needs to rename the exe and create files beside it).
Failures are written to the log (`%APPDATA%\PdfCraft\data\logs\pdfcraft.log`), never shown.

The checksum protects against a damaged or truncated download. The trust anchor is HTTPS to
`github.com/killergan/pdfcraft`: whoever can publish a release there can change what every
installed copy runs.

Upstream's README says "no telemetry". The update check is one request to the GitHub API
(it sends the app name and version in the User-Agent, like any HTTP client).
