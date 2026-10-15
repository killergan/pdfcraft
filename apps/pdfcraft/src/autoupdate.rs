//! BEM fork: silent self-update from this fork's GitHub releases (Windows).
//!
//! A release built by `.github/workflows/bem-release.yml` carries the build label `bem-<N>`
//! (`PDFCRAFT_BUILD_LABEL`) and is published as the release tagged `bem-<N>`. A little after
//! start, a background thread asks GitHub for the fork's latest release. If its number is higher
//! it downloads `pdfcraft-windows-x86_64.exe`, checks it against the release's `.sha256` file and
//! swaps it in for the running executable (Windows lets a running .exe be renamed). The new
//! version runs from the next launch. There is no dialog, prompt or consent step.
//!
//! A build without that label (anything made with plain `cargo build`) never updates itself, so a
//! development build can't overwrite itself with a release. `PDFCRAFT_NO_AUTO_UPDATE` (any
//! value) turns it off. Everything fetched is untrusted: sizes are capped, URLs must be this
//! fork's release downloads, and any failure is logged and ignored.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// The fork whose releases are followed.
const REPO: &str = "killergan/pdfcraft";
const EXE_ASSET: &str = "pdfcraft-windows-x86_64.exe";
const SUM_ASSET: &str = "pdfcraft-windows-x86_64.exe.sha256";
/// The largest executable accepted (the real one is around a tenth of this).
const MAX_EXE: u64 = 400 << 20;
const USER_AGENT: &str = concat!("PdfCraft/", env!("CARGO_PKG_VERSION"), " auto-update");

/// A release newer than this build, with the two files to fetch.
#[derive(Debug, PartialEq, Eq)]
struct Candidate {
    number: u64,
    exe_url: String,
    sum_url: String,
}

/// `bem-12` is build 12; anything else is no build number.
fn build_number(label: Option<&str>) -> Option<u64> {
    label?.trim().strip_prefix("bem-")?.parse().ok()
}

/// The release in `body` (GitHub's `releases/latest` answer) when it is newer than build `current`.
fn candidate(body: &str, current: u64) -> Result<Option<Candidate>, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("unreadable answer ({e})"))?;
    let Some(number) = v["tag_name"].as_str().and_then(|t| build_number(Some(t))) else {
        return Ok(None);
    };
    if number <= current {
        return Ok(None);
    }
    let prefix = format!("https://github.com/{REPO}/releases/download/");
    let asset = |name: &str| -> Option<String> {
        let found = v["assets"].as_array()?.iter().find(|a| a["name"].as_str() == Some(name))?;
        found["browser_download_url"].as_str().filter(|u| u.starts_with(&prefix)).map(str::to_owned)
    };
    match (asset(EXE_ASSET), asset(SUM_ASSET)) {
        (Some(exe_url), Some(sum_url)) => Ok(Some(Candidate { number, exe_url, sum_url })),
        _ => Err(format!("release bem-{number} has no {EXE_ASSET} with a checksum")),
    }
}

/// The SHA-256 in a `sha256sum`-style file (`<hex>  <name>`), lowercase.
fn parse_checksum(text: &str) -> Option<String> {
    let hex = text.split_whitespace().next()?.to_ascii_lowercase();
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hex)
}

/// `pdfcraft.exe` → `pdfcraft.exe.new`.
fn beside(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn agent(timeout: Duration) -> Result<ureq::Agent, String> {
    let roots = crate::updates::os_roots()?;
    Ok(ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .tls_config(ureq::tls::TlsConfig::builder().root_certs(roots).build())
        .build()
        .new_agent())
}

fn get_text(agent: &ureq::Agent, url: &str, limit: u64) -> Result<String, String> {
    let mut response = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("couldn't reach GitHub ({e})"))?;
    response.body_mut().with_config().limit(limit).read_to_string().map_err(|e| format!("unreadable answer ({e})"))
}

/// Streams `url` into `dest`, returning the SHA-256 (lowercase hex) of what was written. The file
/// is created first, so a folder that can't be written to fails before anything is downloaded.
fn download(agent: &ureq::Agent, url: &str, dest: &Path) -> Result<String, String> {
    let mut file = std::fs::File::create(dest).map_err(|e| format!("can't write {}: {e}", dest.display()))?;
    let mut response = agent.get(url).header("User-Agent", USER_AGENT).call().map_err(|e| format!("download failed ({e})"))?;
    let mut reader = response.body_mut().with_config().limit(MAX_EXE).reader();
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("download interrupted ({e})"))?;
        if n == 0 {
            break;
        }
        let chunk = buf.get(..n).ok_or("download interrupted (short read)")?;
        hasher.update(chunk);
        file.write_all(chunk).map_err(|e| format!("can't write {}: {e}", dest.display()))?;
    }
    file.flush().map_err(|e| format!("can't write {}: {e}", dest.display()))?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Puts `new` where the running `exe` is, keeping the old one as `old` until the next start.
fn install(exe: &Path, new: &Path, old: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(old);
    std::fs::rename(exe, old).map_err(|e| format!("can't move the running program aside: {e}"))?;
    if let Err(e) = std::fs::rename(new, exe) {
        // Put things back so the app still starts.
        let _ = std::fs::rename(old, exe);
        return Err(format!("can't put the new version in place: {e}"));
    }
    Ok(())
}

/// One update attempt: `Ok(Some(n))` when build `n` was installed.
fn check() -> Result<Option<u64>, String> {
    let Some(current) = build_number(pdfcraft_engine::build_info::LABEL) else {
        return Ok(None);
    };
    let exe = std::env::current_exe().map_err(|e| format!("no executable path: {e}"))?;
    let (new, old) = (beside(&exe, ".new"), beside(&exe, ".old"));
    // Left by the previous update (still locked, then) or by an interrupted download.
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::remove_file(&new);

    let latest = get_text(&agent(Duration::from_secs(15))?, &format!("https://api.github.com/repos/{REPO}/releases/latest"), 1 << 20)?;
    let Some(release) = candidate(&latest, current)? else {
        return Ok(None);
    };
    let downloads = agent(Duration::from_secs(900))?;
    let wanted = parse_checksum(&get_text(&downloads, &release.sum_url, 4096)?).ok_or("the release's checksum file is not a SHA-256")?;
    let got = download(&downloads, &release.exe_url, &new);
    let outcome = match got {
        Ok(got) if got == wanted => install(&exe, &new, &old),
        Ok(got) => Err(format!("checksum mismatch (expected {wanted}, got {got})")),
        Err(e) => Err(e),
    };
    if outcome.is_err() {
        let _ = std::fs::remove_file(&new);
    }
    outcome.map(|()| Some(release.number))
}

/// Starts the check on its own thread, a little after start so it never competes with the window
/// coming up. Does nothing for an unlabelled build or when `PDFCRAFT_NO_AUTO_UPDATE` is set.
pub fn spawn() {
    if std::env::var_os("PDFCRAFT_NO_AUTO_UPDATE").is_some() || build_number(pdfcraft_engine::build_info::LABEL).is_none() {
        return;
    }
    let started = std::thread::Builder::new().name("auto-update".into()).spawn(|| {
        std::thread::sleep(Duration::from_secs(20));
        match check() {
            Ok(Some(n)) => log::info!("auto-update: build bem-{n} installed, it runs from the next start"),
            Ok(None) => log::debug!("auto-update: up to date"),
            Err(e) => log::warn!("auto-update: {e}"),
        }
    });
    if let Err(e) = started {
        log::warn!("auto-update: couldn't start: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUM: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn release(tag: &str, exe: &str, sum: &str) -> String {
        format!(
            r#"{{"tag_name":"{tag}","assets":[
                {{"name":"{EXE_ASSET}","browser_download_url":"{exe}"}},
                {{"name":"{SUM_ASSET}","browser_download_url":"{sum}"}}]}}"#
        )
    }

    fn url(name: &str, tag: &str) -> String {
        format!("https://github.com/{REPO}/releases/download/{tag}/{name}")
    }

    #[test]
    fn only_labelled_bem_builds_have_a_number() {
        assert_eq!(build_number(Some("bem-12")), Some(12));
        assert_eq!(build_number(Some(" bem-3 ")), Some(3));
        for no in [None, Some(""), Some("titlebar"), Some("bem-"), Some("bem-x"), Some("v0.8.0")] {
            assert_eq!(build_number(no), None, "{no:?}");
        }
    }

    #[test]
    fn a_newer_release_with_both_files_is_offered() {
        let body = release("bem-13", &url(EXE_ASSET, "bem-13"), &url(SUM_ASSET, "bem-13"));
        let found = candidate(&body, 12).unwrap().unwrap();
        assert_eq!(found.number, 13);
        assert_eq!(found.exe_url, url(EXE_ASSET, "bem-13"));
        assert_eq!(candidate(&body, 13).unwrap(), None, "same build");
        assert_eq!(candidate(&body, 14).unwrap(), None, "never a downgrade");
    }

    #[test]
    fn other_releases_and_other_hosts_are_refused() {
        let upstream = release("v0.9.0", &url(EXE_ASSET, "v0.9.0"), &url(SUM_ASSET, "v0.9.0"));
        assert_eq!(candidate(&upstream, 1).unwrap(), None, "not a bem tag");
        let elsewhere = release("bem-13", "https://example.com/pdfcraft.exe", &url(SUM_ASSET, "bem-13"));
        assert!(candidate(&elsewhere, 1).is_err(), "download must come from this fork's releases");
        assert!(candidate("{}", 1).unwrap().is_none());
        assert!(candidate("not json", 1).is_err());
    }

    #[test]
    fn the_checksum_file_must_hold_a_sha256() {
        assert_eq!(parse_checksum(&format!("{SUM}  {EXE_ASSET}\n")).as_deref(), Some(SUM));
        assert_eq!(parse_checksum(&SUM.to_uppercase()).as_deref(), Some(SUM));
        assert_eq!(parse_checksum("deadbeef"), None);
        assert_eq!(parse_checksum(""), None);
        assert_eq!(parse_checksum(&format!("{}g", &SUM[..63])), None);
    }

    #[test]
    fn install_swaps_the_file_and_puts_it_back_when_it_cannot() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-autoupdate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (exe, new, old) = (dir.join("pdfcraft.exe"), dir.join("pdfcraft.exe.new"), dir.join("pdfcraft.exe.old"));
        std::fs::write(&exe, b"v1").unwrap();
        std::fs::write(&new, b"v2").unwrap();
        install(&exe, &new, &old).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"v2");
        assert_eq!(std::fs::read(&old).unwrap(), b"v1");
        // The new file is missing: the program must still be where it was.
        assert!(install(&exe, &new, &old).is_err());
        assert_eq!(std::fs::read(&exe).unwrap(), b"v2");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
