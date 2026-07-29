use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::github::{self, ReleaseAsset};

pub(super) async fn download_asset(
    asset: &ReleaseAsset,
    destination: &Path,
    timeout: Duration,
    show_progress: bool,
) -> Result<()> {
    let response = response_for_url(&asset.download_url, timeout).await?;
    let response = response
        .error_for_status()
        .with_context(|| format!("GitHub asset download returned an error for {}", asset.name))?;
    let total = response.content_length();
    let mut stream = response;
    let mut file = tokio::fs::File::create(destination)
        .await
        .with_context(|| format!("failed to create downloaded archive {}", destination.display()))?;
    let mut downloaded = 0_u64;

    while let Some(chunk) = stream
        .chunk()
        .await
        .with_context(|| format!("failed while downloading {}", asset.name))?
    {
        file.write_all(&chunk)
            .await
            .with_context(|| format!("failed to write downloaded archive {}", destination.display()))?;
        downloaded += chunk.len() as u64;
        if show_progress {
            print_progress(&asset.name, downloaded, total);
        }
    }
    file.flush().await.context("failed to flush downloaded archive")?;
    if show_progress {
        println!();
    }
    Ok(())
}

pub(super) async fn download_checksum(asset: &ReleaseAsset, timeout: Duration) -> Result<String> {
    let response = response_for_url(&asset.download_url, timeout).await?;
    response
        .error_for_status()
        .with_context(|| format!("checksum download returned an error for {}", asset.name))?
        .text()
        .await
        .with_context(|| format!("failed to read checksum metadata {}", asset.name))
}

async fn response_for_url(url: &str, timeout: Duration) -> Result<reqwest::Response> {
    let response = github::github_client()?
        .get(url)
        .timeout(timeout)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await
        .with_context(|| format!("failed to download update asset from {url}"));

    match response {
        Ok(response) if response.status() == reqwest::StatusCode::UNAUTHORIZED && github::github_token().is_some() => {
            github::unauthenticated_client()?
                .get(url)
                .timeout(timeout)
                .header(reqwest::header::ACCEPT, "application/octet-stream")
                .send()
                .await
                .with_context(|| format!("failed to download update asset from {url}"))
        }
        Ok(response) => Ok(response),
        Err(error) => Err(error),
    }
}

pub(super) fn checksum_asset<'a>(assets: &'a [ReleaseAsset], archive_name: &str) -> Option<&'a ReleaseAsset> {
    let archive_name = archive_name.to_ascii_lowercase();
    // Modern sidecar: `archive.tar.gz.sha256` / `archive.zip.sha256`.
    let modern_sidecar = format!("{archive_name}.sha256");
    // Legacy extension-stripped sidecar: `vtcode-<v>-<target>.sha256`
    // (the `.tar.gz`/`.zip` suffix removed). This is the convention the
    // release pipeline publishes today, so prefer it over the aggregate file.
    let legacy_sidecar = legacy_checksum_name(&archive_name);

    assets
        .iter()
        .find(|asset| asset.name.to_ascii_lowercase() == modern_sidecar)
        .or_else(|| {
            let legacy = legacy_sidecar.as_deref();
            assets
                .iter()
                .find(|asset| legacy.is_some_and(|name| asset.name.to_ascii_lowercase() == name))
        })
        .or_else(|| {
            assets.iter().find(|asset| {
                let name = asset.name.to_ascii_lowercase();
                name == "checksums.txt" || name == "sha256sums.txt"
            })
        })
}

/// Derive the legacy extension-stripped checksum sidecar name for an archive.
///
/// `vtcode-1.0.0-aarch64-apple-darwin.tar.gz` -> `vtcode-1.0.0-aarch64-apple-darwin.sha256`.
/// Returns `None` for archives that are neither `.tar.gz` nor `.zip`.
fn legacy_checksum_name(archive_name: &str) -> Option<String> {
    archive_name
        .strip_suffix(".tar.gz")
        .or_else(|| archive_name.strip_suffix(".zip"))
        .map(|stem| format!("{stem}.sha256"))
}

pub(super) fn parse_checksum_metadata(metadata: &str, archive_name: &str) -> Option<String> {
    let archive_name = archive_name.to_ascii_lowercase();
    metadata.lines().find_map(|line| {
        let tokens: Vec<_> = line.split_whitespace().collect();
        let digest = tokens.iter().find(|token| is_sha256(token))?;
        if tokens.len() == 1 || line.to_ascii_lowercase().contains(&archive_name) {
            Some(digest.to_ascii_lowercase())
        } else {
            None
        }
    })
}

pub(super) fn verify_checksum(contents: &[u8], expected: &str) -> Result<()> {
    if !is_sha256(expected) {
        bail!("invalid SHA-256 checksum metadata");
    }
    let actual = digest_hex(contents);
    if actual != expected.to_ascii_lowercase() {
        bail!("checksum mismatch: expected {expected}, got {actual}");
    }
    Ok(())
}

pub(super) fn verify_file_checksum(path: &Path, expected: &str) -> Result<()> {
    let contents =
        std::fs::read(path).with_context(|| format!("failed to read {} for checksum verification", path.display()))?;
    verify_checksum(&contents, expected)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn digest_hex(contents: &[u8]) -> String {
    Sha256::digest(contents).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn print_progress(name: &str, downloaded: u64, total: Option<u64>) {
    let mut output = std::io::stdout().lock();
    match total {
        Some(total) if total > 0 => {
            let percent = downloaded.saturating_mul(100) / total;
            let _ = write!(output, "\rDownloading {name}: {percent}% ({downloaded}/{total} bytes)");
        }
        _ => {
            let _ = write!(output, "\rDownloading {name}: {downloaded} bytes");
        }
    }
    let _ = output.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_checksum_for_named_archive() {
        let checksum = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  vtcode-1.0.0.tar.gz\n";
        assert_eq!(
            parse_checksum_metadata(checksum, "vtcode-1.0.0.tar.gz"),
            Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string())
        );
    }

    #[test]
    fn ignores_checksum_for_another_archive() {
        let checksum = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  other.tar.gz\n";
        assert!(parse_checksum_metadata(checksum, "vtcode.tar.gz").is_none());
    }

    #[test]
    fn verifies_checksum_and_rejects_mismatch() {
        let expected = digest_hex(b"archive");
        verify_checksum(b"archive", &expected).expect("checksum");
        let error = verify_checksum(b"tampered", &expected).expect_err("mismatch");
        assert!(error.to_string().contains("checksum mismatch"));
    }

    #[test]
    fn missing_checksum_metadata_is_optional() {
        assert!(parse_checksum_metadata("signature only", "vtcode.tar.gz").is_none());
    }

    #[test]
    fn ignores_unrelated_architecture_checksum_sidecars() {
        let assets = vec![
            ReleaseAsset {
                name: "vtcode-1.0.0-aarch64-apple-darwin.tar.gz".to_string(),
                download_url: "https://example.test/archive".to_string(),
            },
            ReleaseAsset {
                name: "vtcode-1.0.0-x86_64-apple-darwin.tar.gz.sha256".to_string(),
                download_url: "https://example.test/checksum".to_string(),
            },
        ];

        assert!(checksum_asset(&assets, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz").is_none());
    }

    #[test]
    fn selects_legacy_extension_stripped_checksum_sidecar() {
        // The release pipeline publishes `vtcode-<v>-<target>.sha256`
        // (extension-stripped) alongside the `.tar.gz` archive.
        let assets = vec![
            ReleaseAsset {
                name: "vtcode-1.0.0-aarch64-apple-darwin.tar.gz".to_string(),
                download_url: "https://example.test/archive".to_string(),
            },
            ReleaseAsset {
                name: "vtcode-1.0.0-aarch64-apple-darwin.sha256".to_string(),
                download_url: "https://example.test/checksum".to_string(),
            },
        ];

        let selected = checksum_asset(&assets, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz").expect("sidecar");

        assert_eq!(selected.name, "vtcode-1.0.0-aarch64-apple-darwin.sha256");
    }

    #[test]
    fn selects_legacy_extension_stripped_checksum_sidecar_for_zip() {
        let assets = vec![
            ReleaseAsset {
                name: "vtcode-1.0.0-x86_64-pc-windows-msvc.zip".to_string(),
                download_url: "https://example.test/archive".to_string(),
            },
            ReleaseAsset {
                name: "vtcode-1.0.0-x86_64-pc-windows-msvc.sha256".to_string(),
                download_url: "https://example.test/checksum".to_string(),
            },
        ];

        let selected = checksum_asset(&assets, "vtcode-1.0.0-x86_64-pc-windows-msvc.zip").expect("sidecar");

        assert_eq!(selected.name, "vtcode-1.0.0-x86_64-pc-windows-msvc.sha256");
    }

    #[test]
    fn prefers_modern_sidecar_over_legacy_stripped_sidecar() {
        let assets = vec![
            ReleaseAsset {
                name: "vtcode-1.0.0-aarch64-apple-darwin.tar.gz.sha256".to_string(),
                download_url: "https://example.test/modern".to_string(),
            },
            ReleaseAsset {
                name: "vtcode-1.0.0-aarch64-apple-darwin.sha256".to_string(),
                download_url: "https://example.test/legacy".to_string(),
            },
        ];

        let selected = checksum_asset(&assets, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz").expect("sidecar");

        assert_eq!(selected.name, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz.sha256");
    }

    #[tokio::test]
    async fn download_reports_http_failures_with_asset_context() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("connection");
            let mut request = [0_u8; 256];
            let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut request).await;
            stream
                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .expect("response");
        });

        let asset = ReleaseAsset {
            name: "vtcode.tar.gz".to_string(),
            download_url: format!("http://{address}/missing"),
        };
        let temp = tempfile::tempdir().expect("temp");
        let error = download_asset(&asset, &temp.path().join("archive"), Duration::from_secs(1), false)
            .await
            .expect_err("http error");

        assert!(error.to_string().contains("vtcode.tar.gz"));
    }

    #[tokio::test]
    async fn download_reports_timeout_with_url_context() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            let _ = listener.accept().await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        });

        let error = response_for_url(&format!("http://{address}/slow"), Duration::from_millis(20))
            .await
            .expect_err("timeout");

        assert!(error.to_string().contains("failed to download update asset"));
    }
}
