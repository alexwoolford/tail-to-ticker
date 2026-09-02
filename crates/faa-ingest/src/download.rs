use std::path::Path;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, SERVER};
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
pub const FAA_DOWNLOAD_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

const GET_ATTEMPTS: u32 = 4;

fn default_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    headers
}

pub fn http_client(user_agent: &str) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(user_agent)
        .gzip(true)
        .default_headers(default_headers())
        .build()?)
}

fn server_name(resp: &reqwest::Response) -> String {
    resp.headers()
        .get(SERVER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

async fn get_success_bytes(client: &reqwest::Client, url: &str) -> Result<bytes::Bytes> {
    let mut delay = Duration::from_secs(2);
    let mut last: Option<(reqwest::StatusCode, String)> = None;
    for attempt in 1..=GET_ATTEMPTS {
        let resp = client.get(url).send().await?;
        let status = resp.status();
        let server = server_name(&resp);
        if status.as_u16() == 503 && attempt < GET_ATTEMPTS {
            tracing::warn!(attempt, %status, server = %server, url, "origin 503; retrying");
            tokio::time::sleep(delay).await;
            delay *= 2;
            last = Some((status, server));
            continue;
        }
        if !status.is_success() {
            return Err(Error::Msg(format!(
                "HTTP {status} for {url} (Server: {server})"
            )));
        }
        return Ok(resp.bytes().await?);
    }
    let (status, server) = last.expect("503 retry always records status");
    Err(Error::Msg(format!(
        "HTTP {status} for {url} (Server: {server}) after {GET_ATTEMPTS} attempts"
    )))
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
