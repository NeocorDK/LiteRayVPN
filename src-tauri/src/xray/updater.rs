//! Updates of the Xray core and geo files from GitHub releases, verified by SHA-256.

use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CORE_REPO: &str = "XTLS/Xray-core";
const CORE_ASSET: &str = "Xray-windows-64.zip";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreRelease {
    pub tag: String,
    pub prerelease: bool,
    pub published_at: String,
    #[serde(skip)]
    zip_url: String,
    #[serde(skip)]
    dgst_url: String,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    prerelease: bool,
    draft: bool,
    published_at: Option<String>,
    assets: Vec<GhAsset>,
}

fn parse_version(tag: &str) -> Option<semver::Version> {
    semver::Version::parse(tag.trim().trim_start_matches('v')).ok()
}

pub fn is_newer(current: Option<&str>, candidate: &str) -> bool {
    match (current.and_then(parse_version), parse_version(candidate)) {
        (Some(cur), Some(new)) => new > cur,
        (None, Some(_)) => true,
        _ => false,
    }
}

pub async fn latest_core_release(client: &reqwest::Client, include_prerelease: bool) -> anyhow::Result<CoreRelease> {
    let releases: Vec<GhRelease> = client
        .get(format!("https://api.github.com/repos/{CORE_REPO}/releases?per_page=15"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    releases
        .into_iter()
        .filter(|r| !r.draft && (include_prerelease || !r.prerelease))
        .filter(|r| parse_version(&r.tag_name).is_some())
        .find_map(|r| {
            let url = |name: &str| r.assets.iter().find(|a| a.name == name).map(|a| a.browser_download_url.clone());
            Some(CoreRelease {
                zip_url: url(CORE_ASSET)?,
                dgst_url: url(&format!("{CORE_ASSET}.dgst"))?,
                tag: r.tag_name.clone(),
                prerelease: r.prerelease,
                published_at: r.published_at.clone().unwrap_or_default(),
            })
        })
        .ok_or_else(|| anyhow::anyhow!("no suitable Xray release found"))
}

async fn download(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    Ok(client.get(url).send().await?.error_for_status()?.bytes().await?.to_vec())
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn verify(data: &[u8], expected: &str, what: &str) -> anyhow::Result<()> {
    let actual = sha256_hex(data);
    if !actual.eq_ignore_ascii_case(expected.trim()) {
        anyhow::bail!("checksum mismatch for {what}");
    }
    Ok(())
}

/// Writes `data` next to `target` and swaps it in, keeping the previous file as `.bak`.
fn replace_file(target: &Path, data: &[u8]) -> anyhow::Result<()> {
    let tmp = target.with_extension("new");
    std::fs::write(&tmp, data)?;
    if target.exists() {
        let bak = target.with_extension(format!("{}.bak", target.extension().and_then(|e| e.to_str()).unwrap_or("")));
        let _ = std::fs::remove_file(&bak);
        std::fs::rename(target, &bak)?;
    }
    std::fs::rename(&tmp, target)?;
    Ok(())
}

/// Downloads the release, verifies it and returns the xray.exe bytes.
pub async fn download_core(client: &reqwest::Client, release: &CoreRelease) -> anyhow::Result<Vec<u8>> {
    let zip_bytes = download(client, &release.zip_url).await?;
    let dgst = String::from_utf8(download(client, &release.dgst_url).await?)?;
    let expected = dgst
        .lines()
        .find_map(|l| l.strip_prefix("SHA2-256="))
        .ok_or_else(|| anyhow::anyhow!("SHA2-256 line missing in .dgst"))?;
    verify(&zip_bytes, expected, CORE_ASSET)?;

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))?;
    let mut exe = Vec::new();
    archive.by_name("xray.exe")?.read_to_end(&mut exe)?;
    Ok(exe)
}

/// Swaps in a downloaded core. The caller must stop xray first.
pub fn apply_core(core_dir: &Path, exe: &[u8], tag: &str) -> anyhow::Result<()> {
    replace_file(&core_dir.join("xray.exe"), exe)?;
    std::fs::write(core_dir.join("VERSION"), tag)?;
    Ok(())
}

/// Downloads geoip.dat and geosite.dat from the latest release of `repo`.
pub async fn update_geo(client: &reqwest::Client, repo: &str, core_dir: &Path) -> anyhow::Result<()> {
    let base = format!("https://github.com/{repo}/releases/latest/download");
    let mut files = Vec::new();
    for name in ["geoip.dat", "geosite.dat"] {
        let data = download(client, &format!("{base}/{name}")).await?;
        let sum = String::from_utf8(download(client, &format!("{base}/{name}.sha256sum")).await?)?;
        let expected = sum.split_whitespace().next().unwrap_or_default();
        verify(&data, expected, name)?;
        files.push((name, data));
    }
    // Replace only after both files verified, so the pair never gets out of sync.
    for (name, data) in files {
        replace_file(&core_dir.join(name), &data)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer(Some("v26.9.9"), "v26.10.17"));
        assert!(!is_newer(Some("v26.9.9"), "v26.3.27"));
        assert!(!is_newer(Some("v26.9.9"), "v26.9.9"));
        assert!(is_newer(None, "v1.0.0"));
        assert!(!is_newer(Some("v1.0.0"), "nightly"));
    }

    #[test]
    fn checksum() {
        assert!(verify(b"abc", "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD", "x").is_ok());
        assert!(verify(b"abd", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", "x").is_err());
    }
}
