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

1. `bem-release.yml` runs daily (and on demand from the Actions tab).
   - **`build` job** (read-only token, no secrets): merges upstream's *latest release tag* (not
     `main`) into this fork's `main` in the runner's checkout, builds `pdfcraft.exe` with
     `PDFCRAFT_BUILD_LABEL=bem-<run number>`, and hands over that one file. It compiles upstream
     code, so it is kept away from the signing key and any write permission.
   - **`publish` job** (the secret and a write token, no upstream code is run): hashes the exe,
     signs the hash file with Ed25519 and publishes release `bem-<run number>` with
     `pdfcraft-windows-x86_64.exe`, `.sha256` and `.sha256.sig`.
   - Nothing is built when neither this fork's `main` nor upstream's release moved since the last
     release. A merge conflict fails the run (GitHub emails you).
   - The fork's `main` is not moved by the run. To bring it up to date, merge upstream's release
     tag into a branch yourself (or use GitHub's "Sync fork") and merge that branch.
2. A labelled build checks the fork's latest release about 20 seconds after start. If the release
   number is higher it fetches the `.sha256` and `.sig`, **verifies the signature against the
   public key built into the app, then** downloads the exe, checks its SHA-256, renames the running
   exe to `pdfcraft.exe.old` and puts the new one in its place. It runs from the next launch. No
   dialog. Anything that fails is logged and ignored.
3. A build without the `bem-<N>` label (plain `cargo build`) never updates itself.
   `PDFCRAFT_NO_AUTO_UPDATE=1` turns updating off for a labelled build.

First install: use the MSI or portable zip built from a release, and keep the exe where the app can
write beside it (the portable zip's folder; for the MSI, the install folder must be writable by
whoever runs it).

## Signing key

- The public half is `PUBLIC_KEY_HEX` in `apps/pdfcraft/src/autoupdate.rs`.
- The private half is the repository secret `BEM_UPDATE_SIGNING_KEY` (an Ed25519 PEM). It exists
  nowhere else.
- To rotate: make a new key with `openssl genpkey -algorithm ED25519 -out key.pem` and read its
  public half with `openssl pkey -in key.pem -pubout -outform DER | tail -c 32 | xxd -p -c 64`.
  Put that in `PUBLIC_KEY_HEX` and publish a release **still signed with the old secret**: installed
  copies only trust the old key, so that is the release that carries the new one to them. Once
  they have updated, replace the secret (`gh secret set BEM_UPDATE_SIGNING_KEY < key.pem`).
  A lost or leaked key with no copy left to update means installing a fresh MSI by hand.

## What this does and doesn't protect

- A release asset that was replaced, or a download from another URL, is refused: it isn't signed
  with the key.
- An old signed build re-published under a newer tag is refused too: the signed checksum file
  states `build: bem-<N>`, which must equal the tag, so installs can't be rolled back that way.
- Someone who can run the workflow, change the workflow file on `main`, or read the secret can
  still publish a release. Anyone who can push to `main` can change what the next build contains.
  Keep the GitHub account protected (two-factor login) and review changes to `main`.
- Upstream's own releases are trusted as much as upstream: the `build` job compiles them. Only
  upstream *releases* are followed, not every commit.
- The exe is not Authenticode-signed, so Windows SmartScreen warns on first run.

Upstream's README says "no telemetry". The update check is one request to the GitHub API
(it sends the app name and version in the User-Agent, like any HTTP client).
