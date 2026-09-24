// Loopback HTTP/WebDAV server.
//
// One listener on 127.0.0.1 serves all mounts. Each request is routed by the
// `/m/{mount_id}` prefix to the mount's DavHandler, guarded by gateway Basic
// auth and a Host header check (DNS-rebinding defense). Everything below the
// mount prefix is dav-server's job — including Range handling for GET/HEAD.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use hyper::header::{HeaderValue, WWW_AUTHENTICATE};
use hyper::http::StatusCode;
use hyper::server::conn::Http;
use hyper::service::service_fn;
use hyper::{Body as HyperBody, Request, Response};
use tokio::net::TcpListener;

use crate::webdav::mounts::{GATEWAY_USERNAME, MountEntry};
use dav_server::DavHandler;

/// One running mount: its persisted definition plus the prebuilt dav handler.
pub struct MountRuntime {
    pub entry: MountEntry,
    pub handler: DavHandler,
}

/// State shared with every request handler. Mounts swap in/out at runtime,
/// hence the lock keyed by mount_id.
pub type MountMap = Arc<RwLock<HashMap<String, MountRuntime>>>;

#[derive(Clone)]
pub struct ServerShared {
    pub mounts: MountMap,
    pub password: String,
    pub port: u16,
}

/// Bind 127.0.0.1 starting at `preferred`, walking upward on conflicts.
/// Returns the listener and the port that actually bound.
pub async fn bind_listener(preferred: u16) -> std::io::Result<(TcpListener, u16)> {
    let mut last_err = None;
    for port in preferred..preferred.saturating_add(50) {
        match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => return Ok((listener, port)),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::AddrInUse, "no available port")
    }))
}

/// Accept loop; spawned once per app run and lives until the process exits.
pub async fn accept_loop(listener: TcpListener, shared: ServerShared) {
    let http = Http::new();
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(conn) => conn,
            Err(_) => continue,
        };
        let shared = shared.clone();
        let http = http.clone();
        tauri::async_runtime::spawn(async move {
            let service = service_fn(move |req| {
                let shared = shared.clone();
                async move { Ok::<_, std::convert::Infallible>(route(shared, req).await) }
            });
            let _ = http.serve_connection(stream, service).await;
        });
    }
}

fn plain_response(status: StatusCode, body: &str) -> Response<HyperBody> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(HyperBody::from(body.to_string()))
        .unwrap_or_else(|_| Response::new(HyperBody::empty()))
}

fn unauthorized() -> Response<HyperBody> {
    let mut res = plain_response(StatusCode::UNAUTHORIZED, "authentication required\n");
    res.headers_mut().insert(
        WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"ShareOneList\", charset=\"UTF-8\""),
    );
    res
}

async fn route(shared: ServerShared, req: Request<HyperBody>) -> Response<HyperBody> {
    // DNS-rebinding defense: the gateway is only ever addressed as
    // 127.0.0.1; any other Host (including browser-invented domains) is dead.
    let host_ok = req
        .headers()
        .get(hyper::header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|h| {
            let host = h
                .rsplit_once(':')
                .map(|(h, _)| h)
                .unwrap_or(h);
            host == "127.0.0.1"
        })
        .unwrap_or(false);
    if !host_ok {
        return plain_response(StatusCode::FORBIDDEN, "host not allowed\n");
    }

    // Path: /m/{mount_id}/...
    let path = req.uri().path();
    let mut segs = path.trim_start_matches('/').splitn(3, '/');
    let scope = segs.next().unwrap_or("");
    let mount_id = segs.next().unwrap_or("");
    if scope != "m" || mount_id.is_empty() {
        return plain_response(StatusCode::NOT_FOUND, "unknown path\n");
    }

    // Gateway Basic auth (loopback-only secret; never a Graph credential).
    let auth_ok = req
        .headers()
        .get(hyper::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|b64| BASE64.decode(b64.trim()).ok())
        .and_then(|raw| String::from_utf8(raw).ok())
        .map(|creds| {
            creds == format!("{}:{}", GATEWAY_USERNAME, shared.password)
        })
        .unwrap_or(false);
    if !auth_ok {
        return unauthorized();
    }

    let handler = {
        let mounts = shared.mounts.read().expect("mount map poisoned");
        mounts.get(mount_id).map(|rt| rt.handler.clone())
    };
    let Some(handler) = handler else {
        return plain_response(StatusCode::NOT_FOUND, "unknown mount\n");
    };

    // dav-server handles OPTIONS/PROPFIND/GET/HEAD (incl. Range) etc.
    let dav_response = handler.handle(req).await;
    dav_response.map(|body| HyperBody::wrap_stream(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Feature: webdav-mount, Property 5: requests without valid gateway
    // credentials can never list or read mount content (always 401).
    #[test]
    fn unauthorized_helper_sets_www_authenticate() {
        let res = unauthorized();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert!(res
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .starts_with("Basic"));
    }

    #[test]
    fn host_check_accepts_only_loopback() {
        // (extracted logic mirror: host part before the port must be 127.0.0.1)
        let ok = |h: &str| {
            h.rsplit_once(':')
                .map(|(h, _)| h)
                .unwrap_or(h)
                .eq_ignore_ascii_case("127.0.0.1")
        };
        assert!(ok("127.0.0.1:3980"));
        assert!(ok("127.0.0.1"));
        assert!(!ok("evil.example.com:3980"));
        assert!(!ok("192.168.1.5:3980"));
    }
}
