use std::path::Path;

use sha2::{Digest, Sha256};

use crate::{Error, Result};

pub const FAA_ZIP_URL: &str = "https://registry.faa.gov/database/ReleasableAircraft.zip";

/// Origin-accepted User-Agent for `registry.faa.gov`.
///
/// Akamai in front of the zip returns **403 AkamaiGHost** for the SEC contact
/// string (reproduced on OCI and a residential laptop, with `Accept` /
/// `Accept-Language` set). A Chrome UA is not used; this Safari-like token
/// reaches Microsoft-IIS (206/200). Override with `FAA_USER_AGENT`. Do not send
/// this string to `www.sec.gov`.
///
/// Keep this UA, gzip on the client, Accept headers, 503×4 backoff, and
/// `Server` in errors in lockstep with `faa-registry-mirror/src/download.rs`.
pub const FAA_DOWNLOAD_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

pub fn http_client(user_agent: &str) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(user_agent)
        .gzip(true)
        .default_headers(http_get::default_headers())
        .build()?)
}

async fn get_success_bytes(client: &reqwest::Client, url: &str) -> Result<bytes::Bytes> {
    http_get::get_success_bytes(client, url)
        .await
        .map_err(Error::Msg)
}

/// Download the nightly Releasable Aircraft zip. Returns bytes and SHA-256 hex.
pub async fn download_registry(user_agent: &str) -> Result<(Vec<u8>, String)> {
    tracing::info!(url = FAA_ZIP_URL, "downloading FAA registry");
    let client = http_client(user_agent)?;
    let bytes = get_success_bytes(&client, FAA_ZIP_URL).await?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha = hex_encode(&hasher.finalize());
    tracing::info!(bytes = bytes.len(), sha256 = %sha, "FAA registry downloaded");
    Ok((bytes.to_vec(), sha))
}

pub async fn download_registry_to(path: &Path, user_agent: &str) -> Result<(Vec<u8>, String)> {
    let (bytes, sha) = download_registry(user_agent).await?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, &bytes)?;
    Ok((bytes, sha))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
