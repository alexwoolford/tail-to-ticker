use std::path::Path;

use sha2::{Digest, Sha256};

use crate::{Error, Result};

pub const FAA_ZIP_URL: &str = "https://registry.faa.gov/database/ReleasableAircraft.zip";

/// Download the nightly Releasable Aircraft zip. Returns bytes and SHA-256 hex.
pub async fn download_registry(user_agent: &str) -> Result<(Vec<u8>, String)> {
    tracing::info!(url = FAA_ZIP_URL, "downloading FAA registry");
    let client = reqwest::Client::builder()
        .user_agent(user_agent)
        .gzip(true)
        .build()?;
    let bytes = client
        .get(FAA_ZIP_URL)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha = hex_encode(&hasher.finalize());
    tracing::info!(bytes = bytes.len(), sha256 = %sha, "FAA registry downloaded");
    Ok((bytes.to_vec(), sha))
}

pub async fn download_registry_to(
    path: &Path,
    user_agent: &str,
    use_cache: bool,
) -> Result<(Vec<u8>, String)> {
    if use_cache && path.exists() {
        let bytes = tokio_read(path)?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha = hex_encode(&hasher.finalize());
        tracing::info!(path = %path.display(), "using cached FAA zip");
        return Ok((bytes, sha));
    }
    let (bytes, sha) = download_registry(user_agent).await?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, &bytes)?;
    Ok((bytes, sha))
}

fn tokio_read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(Error::from)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
