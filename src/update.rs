//! Updates from the GitHub releases of krosshair: whether there is a newer version, and putting it
//! in place of this exe.
//!
//! A release is tagged `vX.Y.Z` and carries `krosshair.exe` and `krosshair.exe.sha256` (both made
//! by `.github/workflows/release.yml`). The new exe is downloaded next to this one and checked
//! against the hash. Windows lets a running exe be renamed, so this one becomes
//! `krosshair.exe.old` and the new one takes its name. The new copy is then started; it waits for
//! this one to quit, which holds the single-instance lock until then, and removes the old exe.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::ffi::c_void;
use std::path::{Path, PathBuf};

/// The newest release that is neither a draft nor a pre-release. `KROSSHAIR_UPDATE_URL`
/// replaces it, for testing against a local server.
const LATEST: &str = "https://api.github.com/repos/kirchesz/krosshair/releases/latest";
const EXE: &str = "krosshair.exe";
const HASH: &str = "krosshair.exe.sha256";
/// A bigger download is not krosshair.
const MAX_DOWNLOAD: usize = 64 << 20;

/// Starts the copy put in place by an update, with the id of the process it replaces.
pub const AFTER_FLAG: &str = "--after-update";

/// A release newer than this build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// "0.2.0", without the tag's "v".
    pub version: String,
    /// The release's page on GitHub, with its notes.
    pub page: String,
    exe: String,
    hash: String,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

/// This build's version.
pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// "v1.2.3" or "1.2.3" as numbers; pre-releases ("1.2.3-beta") and anything else are not
/// versions to update to.
fn version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.strip_prefix('v').unwrap_or(text).split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// The newest release, if it is newer than this build. `None` also when the repository has no
/// release yet.
pub fn check() -> Result<Option<Release>> {
    let url = std::env::var("KROSSHAIR_UPDATE_URL").unwrap_or_else(|_| LATEST.to_owned());
    let (status, body) = get(&url, true)?;
    match status {
        200 => newer(&body, current()),
        404 => Ok(None),
        _ => bail!("GitHub answered {status}"),
    }
}

/// The release in a GitHub API answer, if it is newer than `current` and has both files.
fn newer(json: &[u8], current: &str) -> Result<Option<Release>> {
    let release: ApiRelease = serde_json::from_slice(json).context("unexpected answer from GitHub")?;
    let (Some(new), Some(now)) = (version(&release.tag_name), version(current)) else { return Ok(None) };
    if release.draft || release.prerelease || new <= now {
        return Ok(None);
    }
    let asset = |name: &str| release.assets.iter().find(|a| a.name == name).map(|a| a.browser_download_url.clone());
    let (Some(exe), Some(hash)) = (asset(EXE), asset(HASH)) else {
        bail!("release {} has no {EXE} or {HASH}", release.tag_name)
    };
    Ok(Some(Release { version: format!("{}.{}.{}", new.0, new.1, new.2), page: release.html_url, exe, hash }))
}

/// The SHA-256 in a hash file: its first word, as `sha256sum` writes it ("<hex>  krosshair.exe").
fn expected_hash(file: &[u8]) -> Result<String> {
    let text = std::str::from_utf8(file).context("the hash file isn't text")?;
    let hash = text.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("the hash file holds no SHA-256");
    }
    Ok(hash)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// `krosshair.exe` → `krosshair.exe.<ext>`, in the same folder.
fn beside(exe: &Path, ext: &str) -> PathBuf {
    let mut name = exe.as_os_str().to_owned();
    name.push(".");
    name.push(ext);
    name.into()
}

/// Downloads the release, checks it and puts it in place of this exe. Returns the exe's path, to
/// start the new one from. On failure this exe stays where it was.
pub fn install(release: &Release) -> Result<PathBuf> {
    let exe = std::env::current_exe().context("can't find krosshair.exe")?;
    let download = |url: &str, what: &str| -> Result<Vec<u8>> {
        let (status, body) = get(url, false)?;
        if status != 200 {
            bail!("downloading {what} failed: GitHub answered {status}");
        }
        Ok(body)
    };
    let bytes = download(&release.exe, EXE)?;
    let expected = expected_hash(&download(&release.hash, HASH)?)?;
    if sha256_hex(&bytes) != expected {
        bail!("the download is damaged: its SHA-256 doesn't match");
    }

    let new = beside(&exe, "new");
    let old = beside(&exe, "old");
    std::fs::write(&new, &bytes).with_context(|| format!("can't write {}", new.display()))?;
    let _ = std::fs::remove_file(&old);
    if let Err(e) = std::fs::rename(&exe, &old) {
        let _ = std::fs::remove_file(&new);
        return Err(e).context("can't move the running krosshair.exe aside");
    }
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        let _ = std::fs::remove_file(&new);
        return Err(e).context("can't put the new krosshair.exe in place");
    }
    Ok(exe)
}

/// Starts the updated exe. It waits for this process to quit before it starts for real.
pub fn start_new(exe: &Path) -> Result<()> {
    std::process::Command::new(exe)
        .arg(AFTER_FLAG)
        .arg(std::process::id().to_string())
        .spawn()
        .context("can't start the new krosshair.exe")?;
    Ok(())
}

/// Started by an update (see [`AFTER_FLAG`]): waits until the copy that was replaced has quit,
/// for up to 20 seconds, then removes its exe.
pub fn after_update(pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    unsafe {
        if let Ok(process) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(process, 20_000);
            let _ = CloseHandle(process);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(beside(&exe, "old"));
        let _ = std::fs::remove_file(beside(&exe, "new"));
    }
}

/// A WinHTTP handle, closed when dropped.
struct Handle(*mut c_void);

impl Handle {
    fn new(raw: *mut c_void, what: &str) -> Result<Self> {
        if raw.is_null() {
            Err(anyhow!(windows::core::Error::from_win32())).context(format!("can't {what}"))
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Networking::WinHttp::WinHttpCloseHandle(self.0);
        }
    }
}

/// A GET with Windows' own HTTP stack (system proxy and certificates, redirects followed):
/// the status code and the body.
fn get(url: &str, api: bool) -> Result<(u32, Vec<u8>)> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Networking::WinHttp::*;

    let (secure, rest) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
        (Some(rest), _) => (true, rest),
        (None, Some(rest)) => (false, rest),
        _ => bail!("not a web address: {url}"),
    };
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let path = if path.is_empty() { "/" } else { path };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().with_context(|| format!("bad port in {url}"))?),
        None => (authority, if secure { 443 } else { 80 }),
    };

    unsafe {
        let agent = HSTRING::from(format!("krosshair/{}", current()));
        let session = Handle::new(
            WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0),
            "open an internet session",
        )?;
        WinHttpSetTimeouts(session.0, 10_000, 10_000, 30_000, 30_000)?;
        let connection = Handle::new(WinHttpConnect(session.0, &HSTRING::from(host), port, 0), "connect")?;
        let flags = if secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let request = Handle::new(
            WinHttpOpenRequest(connection.0, w!("GET"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), flags),
            "open a request",
        )?;
        let headers: Vec<u16> =
            if api { "Accept: application/vnd.github+json\r\n" } else { "Accept: application/octet-stream\r\n" }.encode_utf16().collect();
        WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0).with_context(|| format!("can't reach {host}"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).with_context(|| format!("no answer from {host}"))?;

        let mut status = 0u32;
        let mut size = size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut size,
            std::ptr::null_mut(),
        )?;

        let mut body = Vec::new();
        let mut chunk = vec![0u8; 64 << 10];
        loop {
            let mut read = 0u32;
            WinHttpReadData(request.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read)
                .with_context(|| format!("the download from {host} broke off"))?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() > MAX_DOWNLOAD {
                bail!("the download from {host} is too big");
            }
        }
        Ok((status, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(tag: &str, assets: &[&str]) -> Vec<u8> {
        let assets: Vec<_> = assets
            .iter()
            .map(|name| serde_json::json!({ "name": name, "browser_download_url": format!("https://example.test/{name}") }))
            .collect();
        serde_json::json!({ "tag_name": tag, "html_url": "https://example.test/release", "assets": assets }).to_string().into_bytes()
    }

    #[test]
    fn versions() {
        assert_eq!(version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(version("0.10.0"), Some((0, 10, 0)));
        assert_eq!(version("v1.2"), None);
        assert_eq!(version("v1.2.3.4"), None);
        assert_eq!(version("v1.2.3-beta"), None);
        assert!(version("0.10.0") > version("0.9.9"), "compared as numbers, not text");
    }

    #[test]
    fn only_newer_releases_with_both_files() {
        let both = [EXE, HASH];
        let found = newer(&answer("v0.2.0", &both), "0.1.0").unwrap().unwrap();
        assert_eq!(found.version, "0.2.0");
        assert_eq!(found.exe, "https://example.test/krosshair.exe");
        assert_eq!(found.hash, "https://example.test/krosshair.exe.sha256");
        assert_eq!(newer(&answer("v0.1.0", &both), "0.1.0").unwrap(), None, "the same version");
        assert_eq!(newer(&answer("v0.0.9", &both), "0.1.0").unwrap(), None, "an older one");
        assert_eq!(newer(&answer("nightly", &both), "0.1.0").unwrap(), None, "not a version");
        assert!(newer(&answer("v0.2.0", &[EXE]), "0.1.0").is_err(), "no hash file");
        assert!(newer(b"<html>", "0.1.0").is_err());
    }

    #[test]
    fn hash_files() {
        let hex = sha256_hex(b"krosshair");
        assert_eq!(hex.len(), 64);
        assert_eq!(expected_hash(format!("{}  krosshair.exe\n", hex.to_uppercase()).as_bytes()).unwrap(), hex);
        assert_eq!(expected_hash(hex.as_bytes()).unwrap(), hex);
        assert!(expected_hash(b"not a hash").is_err());
    }

    #[test]
    fn files_beside_the_exe() {
        assert_eq!(beside(Path::new(r"C:\apps\krosshair.exe"), "old"), PathBuf::from(r"C:\apps\krosshair.exe.old"));
    }
}
