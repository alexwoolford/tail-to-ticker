//! GET with shared Accept headers; retry origin 503 with backoff.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, SERVER};

const GET_ATTEMPTS: u32 = 4;

pub fn default_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    headers
}

fn server_name(resp: &reqwest::Response) -> String {
    resp.headers()
        .get(SERVER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

pub async fn get_success_bytes(
    client: &reqwest::Client,
    url: &str,
) -> Result<bytes::Bytes, String> {
    let mut delay = Duration::from_secs(2);
    let mut last: Option<(reqwest::StatusCode, String)> = None;
    for attempt in 1..=GET_ATTEMPTS {
        let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
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
            return Err(format!("HTTP {status} for {url} (Server: {server})"));
        }
        return resp.bytes().await.map_err(|e| e.to_string());
    }
    let (status, server) = last.expect("503 retry always records status");
    Err(format!(
        "HTTP {status} for {url} (Server: {server}) after {GET_ATTEMPTS} attempts"
    ))
}
