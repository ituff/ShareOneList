//! Loopback receiver for the in-webview meeting-recording stream pipeline.
//!
//! SharePoint Stream recordings cannot be downloaded through Graph when tenant
//! policy blocks downloads (`@microsoft.graph.downloadUrl` is withheld), yet
//! the in-app preview player can still play them. The player page inside our
//! own webview fetches a DASH manifest and AES-encrypted segments using its
//! browser-session cookies and a short-lived `x-spopactoken` bearer — none of
//! which a desktop backend can mint (see `src/stream_boot.js` for the port of
//! ms-teams-sharepoint-downloader's technique). The finished MP4 therefore
//! comes back to Rust over this one-shot loopback HTTP channel.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// How long the server waits for the webview to finish downloading and
/// muxing segments before shutting the channel down. Long recordings can
/// take a while at conservative download concurrency, so this must comfortably
/// exceed the whole segment phase.
const CHANNEL_TIMEOUT: Duration = Duration::from_secs(45 * 60);

/// Upper bound for a request head (request line + headers).
const MAX_HEAD_BYTES: usize = 32 * 1024;

/// Channel details handed to the webview so it can deliver the finished file.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamDownloadChannel {
    pub port: u16,
    pub upload_token: String,
}

/// Spawns a one-shot loopback upload server bound to 127.0.0.1:{random port}.
/// The server accepts CORS-preflight OPTIONS requests and a single
/// `POST /upload?token={upload_token}` carrying the finished MP4 as the body.
/// It shuts down after the upload completes or after `CHANNEL_TIMEOUT`.
pub async fn start_upload_server(save_path: PathBuf) -> Result<StreamDownloadChannel, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind loopback listener: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to resolve loopback port: {}", e))?
        .port();
    let upload_token = uuid::Uuid::new_v4().to_string();
    let channel = StreamDownloadChannel {
        port,
        upload_token: upload_token.clone(),
    };

    // `uploading` guards against concurrent uploads on one channel; the file
    // is written only on a token-verified POST, so partial writes never land
    // (the webview POSTs once, only after the full mux succeeds).
    tokio::spawn(async move {
        let mut uploading = false;
        let deadline = tokio::time::Instant::now() + CHANNEL_TIMEOUT;
        loop {
            let accepted = tokio::time::timeout_at(deadline, listener.accept()).await;
            match accepted {
                Ok(Ok((stream, _))) => {
                    let outcome = handle_connection(stream, &upload_token, &save_path, &mut uploading).await;
                    if outcome == ConnectionOutcome::Done {
                        break;
                    }
                }
                _ => break, // channel timed out or listener failed
            }
        }
    });

    Ok(channel)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionOutcome {
    /// Not the final upload — keep accepting (e.g. CORS preflight).
    KeepListening,
    /// Upload finished (or channel should stop for any other reason).
    Done,
}

async fn handle_connection(
    mut stream: TcpStream,
    upload_token: &str,
    save_path: &PathBuf,
    uploading: &mut bool,
) -> ConnectionOutcome {
    // Body bytes that arrived in the same TCP read as the request head must
    // be preserved - the socket cursor already consumed them.
    let (head, leftover) = match read_head(&mut stream).await {
        Some(v) => v,
        None => return ConnectionOutcome::KeepListening,
    };

    let (line, headers_block) = match head.split_once("\r\n") {
        Some(v) => v,
        None => return ConnectionOutcome::KeepListening,
    };
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");

    if method == "OPTIONS" {
        let _ = write_response(
            &mut stream,
            204,
            "No Content",
            &[("Access-Control-Allow-Origin", "*"),
              ("Access-Control-Allow-Methods", "POST, OPTIONS"),
              ("Access-Control-Allow-Headers", "Content-Type"),
              ("Access-Control-Allow-Private-Network", "true"),
              ("Access-Control-Max-Age", "600")],
            None,
        )
        .await;
        return ConnectionOutcome::KeepListening;
    }

    if method != "POST" || !path.starts_with("/upload") {
        let _ = write_response(
            &mut stream,
            404,
            "Not Found",
            &[("Access-Control-Allow-Origin", "*")],
            Some(b"not found"),
        )
        .await;
        return ConnectionOutcome::KeepListening;
    }

    if *uploading {
        let _ = write_response(
            &mut stream,
            409,
            "Conflict",
            &[("Access-Control-Allow-Origin", "*")],
            Some(b"upload already in progress"),
        )
        .await;
        return ConnectionOutcome::KeepListening;
    }

    let supplied = url_query_param(path, "token").unwrap_or_default();
    if supplied != upload_token {
        let _ = write_response(
            &mut stream,
            403,
            "Forbidden",
            &[("Access-Control-Allow-Origin", "*")],
            Some(b"invalid token"),
        )
        .await;
        return ConnectionOutcome::KeepListening;
    }

    let content_length: u64 = headers_block
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<u64>().ok())?
        })
        .unwrap_or(0);
    if content_length == 0 {
        let _ = write_response(
            &mut stream,
            400,
            "Bad Request",
            &[("Access-Control-Allow-Origin", "*")],
            Some(b"empty body"),
        )
        .await;
        return ConnectionOutcome::KeepListening;
    }

    *uploading = true;
    let write_result = stream_body_to_file(&mut stream, leftover, content_length, save_path).await;
    *uploading = false;

    match write_result {
        Ok(()) => {
            let _ = write_response(
                &mut stream,
                200,
                "OK",
                &[("Access-Control-Allow-Origin", "*")],
                Some(b"saved"),
            )
            .await;
            ConnectionOutcome::Done
        }
        Err(e) => {
            eprintln!("[stream-bridge] Failed to write recording: {}", e);
            let _ = write_response(
                &mut stream,
                500,
                "Internal Server Error",
                &[("Access-Control-Allow-Origin", "*")],
                Some(b"write failed"),
            )
            .await;
            ConnectionOutcome::Done
        }
    }
}

/// Reads bytes until the end of the HTTP head (CRLF CRLF), capped. Returns
/// the head plus any body bytes that were already read past it.
async fn read_head(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buf: Vec<u8> = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    loop {
        if buf.len() > MAX_HEAD_BYTES {
            return None;
        }
        let n = match stream.read(&mut chunk).await {
            Ok(0) => return None,
            Ok(n) => n,
            Err(e) => {
                eprintln!("[stream-bridge] read_head failed: {} ({:?})", e, e.kind());
                return None;
            }
        };
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_double_crlf(&buf) {
            let head = String::from_utf8_lossy(&buf[..pos]).into_owned();
            let leftover = buf[pos + 4..].to_vec();
            return Some((head, leftover));
        }
    }
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Streams exactly `content_length` body bytes into the destination file.
async fn stream_body_to_file(
    stream: &mut TcpStream,
    leftover: Vec<u8>,
    content_length: u64,
    save_path: &PathBuf,
) -> Result<(), String> {
    let mut file = tokio::fs::File::create(save_path)
        .await
        .map_err(|e| format!("{}: {}", save_path.display(), e))?;

    let mut remaining = content_length;
    if !leftover.is_empty() {
        let take = (leftover.len() as u64).min(remaining) as usize;
        file.write_all(&leftover[..take])
            .await
            .map_err(|e| format!("write file: {}", e))?;
        remaining -= take as u64;
    }
    let mut chunk = vec![0u8; 256 * 1024];
    while remaining > 0 {
        let want = chunk.len().min(remaining as usize);
        let n = stream
            .read(&mut chunk[..want])
            .await
            .map_err(|e| format!("read body: {}", e))?;
        if n == 0 {
            return Err("connection closed before full body arrived".to_string());
        }
        file.write_all(&chunk[..n])
            .await
            .map_err(|e| format!("write file: {}", e))?;
        remaining -= n as u64;
    }
    file.flush()
        .await
        .map_err(|e| format!("flush: {}", e))?;
    Ok(())
}

fn url_query_param(path_and_query: &str, name: &str) -> Option<String> {
    let query = path_and_query.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

async fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", status, reason);
    for (name, value) in headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    let body_bytes = body.unwrap_or(b"");
    head.push_str(&format!("Content-Length: {}\r\n", body_bytes.len()));
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes()).await?;
    if !body_bytes.is_empty() {
        stream.write_all(body_bytes).await?;
    }
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Built from hex bytes so no backslash escape sequences appear in this
    // file: tooling layers have been observed mangling CRLF escape sequences.

    const CR: u8 = 0x0D;
    const LF: u8 = 0x0A;

    fn crlf() -> Vec<u8> {
        vec![CR, LF]
    }

    #[tokio::test]
    async fn round_trip_preflight_upload_and_token_reject() {
        let dir = std::env::temp_dir().join(format!("sol-stream-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.mp4");
        let channel = start_upload_server(path.clone()).await.unwrap();

        // CORS preflight must succeed and allow the private-network request.
        let preflight: Vec<u8> = [
            b"OPTIONS /upload HTTP/1.1".as_slice(),
            &crlf(),
            b"Host: x".as_slice(),
            &crlf(),
            b"Origin: https://contoso.sharepoint.com".as_slice(),
            &crlf(),
            b"Access-Control-Request-Method: POST".as_slice(),
            &crlf(),
            &crlf(),
        ]
        .concat();
        let mut s = TcpStream::connect(("127.0.0.1", channel.port)).await.unwrap();
        s.write_all(&preflight).await.unwrap();
        let mut buf = vec![0u8; 2048];
        let n = s.read(&mut buf).await.unwrap();
        let head = String::from_utf8_lossy(&buf[..n]).into_owned();
        assert!(head.starts_with("HTTP/1.1 204"), "preflight head: {}", head);
        assert!(head.to_ascii_lowercase().contains("access-control-allow-private-network: true"));
        s.shutdown().await.unwrap();

        // A wrong token is rejected without touching the destination file.
        let bad: Vec<u8> = [
            b"POST /upload?token=wrong HTTP/1.1".as_slice(),
            &crlf(),
            b"Content-Length: 3".as_slice(),
            &crlf(),
            &crlf(),
            b"abc".as_slice(),
        ]
        .concat();
        let mut s = TcpStream::connect(("127.0.0.1", channel.port)).await.unwrap();
        s.write_all(&bad).await.unwrap();
        let n = s.read(&mut buf).await.unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 403"));
        s.shutdown().await.unwrap();

        // The real upload: head and body arrive in one TCP segment, which
        // exercises the leftover-bytes path in read_head.
        let body: Vec<u8> = (0..500_000u32).map(|i| (i % 251) as u8).collect();
        let head_text = format!(
            "POST /upload?token={} HTTP/1.1{}Content-Type: application/octet-stream{}Content-Length: {}{}{}",
            channel.upload_token,
            String::from_utf8(crlf()).unwrap(),
            String::from_utf8(crlf()).unwrap(),
            body.len(),
            String::from_utf8(crlf()).unwrap(),
            String::from_utf8(crlf()).unwrap(),
        );
        let mut req = head_text.into_bytes();
        req.extend_from_slice(&body);
        let mut s = TcpStream::connect(("127.0.0.1", channel.port)).await.unwrap();
        s.write_all(&req).await.unwrap();
        let n = s.read(&mut buf).await.unwrap();
        let head = String::from_utf8_lossy(&buf[..n]).into_owned();
        assert!(head.starts_with("HTTP/1.1 200"), "upload head: {}", head);
        s.shutdown().await.unwrap();

        let saved = std::fs::read(&path).unwrap();
        assert_eq!(saved, body, "saved file must match uploaded bytes");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
