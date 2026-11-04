// GraphDavFileSystem: translates WebDAV filesystem operations into
// Microsoft Graph calls for one mount point.
//
// Write support (P1): PUT stores file content (simple upload ≤16 MiB,
// resumable session above), MKCOL creates folders, DELETE moves to the
// OneDrive recycle bin, MOVE/COPY map to Graph PATCH/copy. Junk names
// (.DS_Store, ._*, Thumbs.db, …) are black-holed on write: dav-server gets
// a success reply but nothing lands in the cloud, keeping Finder/Explorer
// droppings invisible. Range-PUT / PATCH-style partial updates are not
// implemented (501).
//
// Token handling reuses AuthModule::get_token_for_account so mounts stay
// isolated per (cloud_env, home_account_id) and refresh silently. HTTP calls
// reuse GraphClient's 5xx retry. Graph access tokens never leave this process.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use serde_json::json;
use tauri::Manager;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::auth::cloud_config::CloudEnvironment;
use crate::auth::AuthModule;
use crate::errors::AppError;
use crate::graph::GraphClient;
use crate::webdav::cache::{CachedItem, PathCache};

use dav_server::davpath::DavPath;
use dav_server::fs::{
    DavDirEntry, DavFile, DavMetaData, DavFileSystem, FsError, FsFuture, FsResult, FsStream,
    OpenOptions, ReadDirMeta,
};

const CHILDREN_PAGE_SIZE: u32 = 999;
const CHILDREN_SELECT: &str = "id,name,size,file,folder,lastModifiedDateTime,eTag,createdDateTime";
/// Read-ahead chunk for file GETs. Each read_bytes() miss is one HTTP request
/// to the (pre-authenticated) download URL, so batch well above dav-server's
/// 256KB read_buf_size to keep large-file copies at ~4 requests per 16MB.
const PREFETCH_CHUNK: usize = 4 * 1024 * 1024;
/// Total budget for one 4MB range fetch. The shared GraphClient defaults to a
/// 30s whole-request timeout, which is too tight for cross-border SharePoint
/// links mid-transfer; reads may legitimately take minutes.
const RANGE_FETCH_TIMEOUT: Duration = Duration::from_secs(300);
/// PUT bodies above this size spill from memory to a temp file before upload.
/// Explorer routinely copies gigabyte videos into mounted drives; buffering
/// those in RAM would balloon the process. 16 MiB keeps typical Office
/// documents and photos fully in memory while capping worst-case usage.
const WRITE_SPILL_THRESHOLD: usize = 16 * 1024 * 1024;
/// One upload-session chunk. Graph's resumable session accepts 0-60 MiB
/// chunks; 4 MiB matches the app's other upload engines (retries stay cheap).
const UPLOAD_CHUNK_SIZE: usize = 4 * 1024 * 1024;
/// Retries per upload-session chunk (1s/2s/4s backoff), mirroring the
/// transfer engine's per-chunk retry policy for transient failures.
const UPLOAD_CHUNK_RETRIES: u32 = 3;
/// Whole-request timeout for one upload-session chunk PUT (uplink-limited
/// connections can take minutes to push 4 MiB).
const UPLOAD_CHUNK_TIMEOUT: Duration = Duration::from_secs(600);

/// Characters percent-encoded inside a single Graph path-addressing segment.
/// Filenames cannot contain `/ \ : * ? " < > |` per our validators, but other
/// clients upload anything, so encode the whole colon-syntax-hostile set.
const GRAPH_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b':')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'|');

/// OS junk files that must never surface in listings. Finder drops
/// `.DS_Store`/AppleDouble files into every folder it touches and Explorer
/// writes `Thumbs.db`/`desktop.ini`; surfacing them produces a stream of
/// confusing phantom entries for the user.
pub fn is_junk_file(name: &str) -> bool {
    matches!(
        name,
        ".DS_Store" | "Thumbs.db" | "desktop.ini" | ".TemporaryItems" | ".Spotlight-V100"
    ) || name.starts_with("._")
}

/// Sort in place: folders first, then files, each group by name
/// (case-insensitive) — mirrors the app's file list ordering.
pub fn sort_children(items: &mut [CachedItem]) {
    items.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// Decoded URL path bytes (as produced by dav-server) -> canonical relative
/// path segments. Mount root is the empty vec.
pub fn decode_segments(path_bytes: &[u8]) -> Vec<String> {
    path_bytes
        .split(|&c| c == b'/')
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

/// `{base}/drives/{drive}/root:/a/b` style URL for a mount-relative path.
/// `root_item_id` empty addresses the drive root directly.
pub fn graph_item_path_url(
    base: &str,
    drive_id: &str,
    root_item_id: &str,
    rel_segments: &[String],
) -> String {
    let root_part = if root_item_id.is_empty() {
        "root".to_string()
    } else {
        format!("items/{}", root_item_id)
    };
    if rel_segments.is_empty() {
        format!("{}/drives/{}/{}", base, drive_id, root_part)
    } else {
        let encoded: Vec<String> = rel_segments
            .iter()
            .map(|s| utf8_percent_encode(s, GRAPH_SEGMENT).to_string())
            .collect();
        // Graph colon syntax: `root:/a/b` or `items/{id}:/a/b`
        format!(
            "{}/drives/{}/{}:/{}",
            base,
            drive_id,
            root_part,
            encoded.join("/")
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Write-path URL / payload builders (pure, unit-tested)
// ─────────────────────────────────────────────────────────────────────────────

/// Simple-upload URL for a mount-relative file path. WebDAV PUT means "store
/// this content", so conflicts replace the existing file — the client already
/// asked the user before overwriting. The `:/content` suffix is what makes
/// Graph treat the PUT as content storage; without it the PUT would target
/// the item's *metadata* and be rejected ("Entity only allows writes with a
/// JSON Content-Type header").
fn write_content_url(base: &str, drive_id: &str, root_item_id: &str, rel_segments: &[String]) -> String {
    format!(
        "{}:/content?@microsoft.graph.conflictBehavior=replace",
        graph_item_path_url(base, drive_id, root_item_id, rel_segments)
    )
}

/// createUploadSession URL for payloads above the simple-upload limit.
fn write_session_url(base: &str, drive_id: &str, root_item_id: &str, rel_segments: &[String]) -> String {
    format!(
        "{}:/createUploadSession",
        graph_item_path_url(base, drive_id, root_item_id, rel_segments)
    )
}

/// One upload-session chunk PUT's Content-Range value (inclusive end).
fn content_range(start: usize, end_exclusive: usize, total: usize) -> String {
    format!("bytes {}-{}/{}", start, end_exclusive - 1, total)
}

/// MKCOL payload: `fail` turns a name conflict into Graph 409, which dav-server
/// surfaces as 405 Method Not Allowed per RFC 4918.
fn create_folder_body(name: &str) -> serde_json::Value {
    json!({
        "name": name,
        "folder": {},
        "@microsoft.graph.conflictBehavior": "fail"
    })
}

/// MOVE payload: one PATCH retargets the name and/or the parent folder.
fn move_patch_body(name: &str, dst_parent_id: &str) -> serde_json::Value {
    json!({
        "name": name,
        "parentReference": { "id": dst_parent_id }
    })
}

/// COPY payload. Graph performs the copy asynchronously (202 Accepted); the
/// item appears in the destination once the server-side job finishes.
fn copy_body(name: &str, dst_parent_id: &str) -> serde_json::Value {
    json!({
        "name": name,
        "parentReference": { "id": dst_parent_id }
    })
}

/// Last-segment junk check: `None` when the path has no segments (mount root)
/// or the name is a normal file, `Some(true)` for junk names.
fn write_target_is_junk(rel_segments: &[String]) -> bool {
    rel_segments.last().is_some_and(|n| is_junk_file(n))
}

/// The ingest decision: a chunk stays in the in-memory buffer only while the
/// running total fits the spill threshold.
fn stays_in_memory(current_len: usize, incoming: usize) -> bool {
    current_len + incoming <= WRITE_SPILL_THRESHOLD
}

/// Map a write-path Graph failure to a dav-server FsError. 409 means a name
/// conflict on create/rename (dav-server maps Exists to 405 for MKCOL and
/// handles MOVE overwrite itself); everything else defers to `fs_error`.
fn write_fs_error(err: &AppError) -> FsError {
    match err {
        AppError::GraphApi {
            status_code: 409, ..
        } => FsError::Exists,
        other => fs_error(other),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Raw Graph payloads (minimal; graph::commands keeps its own private ones)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct RawItem {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    file: Option<RawFileFacet>,
    #[serde(default)]
    folder: Option<serde_json::Value>,
    #[serde(rename = "eTag", default)]
    e_tag: Option<String>,
    #[serde(rename = "lastModifiedDateTime", default)]
    last_modified_date_time: Option<String>,
    #[serde(rename = "createdDateTime", default)]
    created_date_time: Option<String>,
}

#[derive(serde::Deserialize)]
struct RawFileFacet {
    #[serde(rename = "mimeType", default)]
    mime_type: Option<String>,
}

#[derive(serde::Deserialize)]
struct RawCollection {
    #[serde(default)]
    value: Vec<RawItem>,
    #[serde(rename = "@odata.nextLink", default)]
    next_link: Option<String>,
}

fn raw_to_cached(raw: RawItem) -> CachedItem {
    CachedItem {
        id: raw.id,
        name: raw.name,
        size: raw.size,
        is_dir: raw.folder.is_some(),
        mime: raw.file.and_then(|f| f.mime_type),
        etag: raw.e_tag,
        modified: raw
            .last_modified_date_time
            .as_deref()
            .and_then(parse_rfc3339),
        created: raw.created_date_time.as_deref().and_then(parse_rfc3339),
    }
}

fn parse_rfc3339(s: &str) -> Option<SystemTime> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| SystemTime::from(dt.with_timezone(&Utc)))
}

/// Pull `@microsoft.graph.downloadUrl` out of a driveItem JSON. An instance
/// annotation property on the item object itself (never nested).
fn extract_download_url(item: &serde_json::Value) -> Option<String> {
    item["@microsoft.graph.downloadUrl"]
        .as_str()
        .map(|s| s.to_string())
}

fn fs_error(err: &AppError) -> FsError {
    match err {
        AppError::GraphApi {
            status_code: 404, ..
        } => FsError::NotFound,
        AppError::GraphApi {
            status_code: 401 | 403,
            ..
        } => FsError::Forbidden,
        AppError::Auth { .. } => FsError::Forbidden,
        _ => FsError::GeneralFailure,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Filesystem implementation
// ─────────────────────────────────────────────────────────────────────────────

/// Everything one mount needs to serve requests. Shared behind an Arc; the
/// Tauri AppHandle gives access to AuthModule state for token refresh.
pub struct FsCtx {
    app: tauri::AppHandle,
    cloud_env: CloudEnvironment,
    home_account_id: String,
    drive_id: String,
    root_item_id: String,
    client: GraphClient,
    cache: PathCache,
}

impl FsCtx {
    fn base_url(&self) -> &str {
        self.client.base_url()
    }

    fn http(&self) -> &reqwest::Client {
        self.client.http_client()
    }

    async fn token(&self) -> Result<String, AppError> {
        let auth = self.app.state::<tokio::sync::Mutex<AuthModule>>();
        let mut auth = auth.lock().await;
        auth.get_token_for_account(self.cloud_env.clone(), &self.home_account_id)
            .await
    }

    /// Authenticated Graph GET with retry; returns parsed JSON.
    async fn get_json(&self, url: &str) -> Result<serde_json::Value, AppError> {
        let token = self.token().await?;
        let current = url.to_string();
        let response = self
            .client
            .request_with_retry(&token, |http, tkn| http.get(&current).bearer_auth(tkn))
            .await?;
        response
            .json()
            .await
            .map_err(|e| AppError::GraphApi {
                message: format!("failed to parse Graph response: {}", e),
                status_code: 0,
            })
    }

    async fn get_children_json(&self, url: &str) -> Result<RawCollection, AppError> {
        let token = self.token().await?;
        let current = url.to_string();
        let response = self
            .client
            .request_with_retry(&token, |http, tkn| http.get(&current).bearer_auth(tkn))
            .await?;
        response
            .json()
            .await
            .map_err(|e| AppError::GraphApi {
                message: format!("failed to parse Graph response: {}", e),
                status_code: 0,
            })
    }

    /// Raw `@microsoft.graph.downloadUrl` for a file item. The URL is
    /// pre-authenticated and short-lived; we fetch it once per open file.
    ///
    /// The item MUST be fetched without `$select`: the download URL is an
    /// OData instance annotation, and a `$select` projection drops it, so
    /// `items/{id}?$select=id,@microsoft.graph.downloadUrl` comes back without
    /// the property and every file read would fail (Explorer then shows all
    /// files as 0-byte placeholders). The full item JSON is tiny; the children
    /// listing in the transfer engine relies on the same full-shape responses.
    async fn fetch_download_url(&self, item_id: &str) -> Result<String, AppError> {
        let url = format!(
            "{}/drives/{}/items/{}",
            self.base_url(),
            self.drive_id,
            item_id
        );
        let json = self.get_json(&url).await?;
        extract_download_url(&json).ok_or_else(|| AppError::GraphApi {
            message: "Graph did not return a download URL".to_string(),
            status_code: 0,
        })
    }

    /// Authenticated Graph write request with 5xx retry. Non-2xx (including
    /// 4xx) surface as `AppError::GraphApi` from `request_with_retry`.
    async fn write_request<F>(&self, build: F) -> Result<reqwest::Response, AppError>
    where
        F: Fn(&reqwest::Client, &str) -> reqwest::RequestBuilder,
    {
        let token = self.token().await?;
        self.client.request_with_retry(&token, build).await
    }

    /// DELETE a drive item by id. Graph moves it to the OneDrive recycle bin —
    /// recoverable, matching the in-app delete behavior.
    async fn delete_item(&self, item_id: &str) -> Result<(), AppError> {
        let url = format!(
            "{}/drives/{}/items/{}",
            self.base_url(),
            self.drive_id,
            item_id
        );
        self.write_request(|http, tkn| http.delete(&url).bearer_auth(tkn))
            .await?;
        Ok(())
    }

    /// POST a JSON body and expect 2xx (201/202/200). Returns the parsed JSON
    /// response when there is one.
    async fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        let response = self
            .write_request(|http, tkn| {
                http.post(url)
                    .bearer_auth(tkn)
                    .header("Content-Type", "application/json")
                    .body(body.to_string())
            })
            .await?;
        response
            .json()
            .await
            .map_err(|e| AppError::GraphApi {
                message: format!("failed to parse Graph write response: {}", e),
                status_code: 0,
            })
    }
}

#[derive(Clone)]
pub struct GraphDavFs {
    ctx: Arc<FsCtx>,
}

impl GraphDavFs {
    pub fn new(
        app: tauri::AppHandle,
        cloud_env: CloudEnvironment,
        home_account_id: String,
        drive_id: String,
        root_item_id: String,
    ) -> Self {
        Self {
            ctx: Arc::new(FsCtx {
                app,
                cloud_env: cloud_env.clone(),
                home_account_id,
                drive_id,
                root_item_id,
                client: GraphClient::new(cloud_env),
                cache: PathCache::new(),
            }),
        }
    }

    fn segments(&self, path: &DavPath) -> Vec<String> {
        decode_segments(path.as_bytes())
    }

    /// Resolve a user-entered mount root path ("/a/b") to its drive item.
    /// Used by the mount creation wizard to pin `root_item_id`.
    pub async fn resolve_root(&self, root_path: &str) -> Result<CachedItem, String> {
        let segs: Vec<String> = root_path
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        self.metadata_segments(&segs)
            .await
            .map_err(|e| format!("{:?}", e))
    }

    /// Metadata for a mount-relative path. Root resolves through the drive
    /// root item (or the configured subtree root) and is cached under "".
    async fn metadata_segments(&self, segs: &[String]) -> Result<CachedItem, FsError> {
        let rel = segs.join("/");
        if let Some(item) = self.ctx.cache.get_item(&rel) {
            return Ok(item);
        }

        // Fast path: the parent listing is fresh. Its presence (or absence)
        // of the name is authoritative — junk files are filtered there, so
        // statting `.DS_Store` correctly 404s without a Graph call.
        if !segs.is_empty() {
            let parent_rel = segs[..segs.len() - 1].join("/");
            if let Some(parent_items) = self.ctx.cache.get_children(&parent_rel) {
                let name = segs.last().expect("segs is non-empty here");
                return match parent_items.into_iter().find(|i| &i.name == name) {
                    Some(item) => {
                        self.ctx.cache.put_item(&rel, item.clone());
                        Ok(item)
                    }
                    None => Err(FsError::NotFound),
                };
            }
        }

        let url = graph_item_path_url(
            self.ctx.base_url(),
            &self.ctx.drive_id,
            &self.ctx.root_item_id,
            segs,
        );
        let json = self.ctx.get_json(&url).await.map_err(|e| fs_error(&e))?;
        let item: CachedItem =
            raw_to_cached(serde_json::from_value(json).map_err(|_| FsError::GeneralFailure)?);
        self.ctx.cache.put_item(&rel, item.clone());
        Ok(item)
    }

    /// Listing for a mount-relative directory, junk-filtered and sorted.
    async fn children_of(&self, segs: &[String]) -> Result<Vec<CachedItem>, FsError> {
        let rel = segs.join("/");
        if let Some(items) = self.ctx.cache.get_children(&rel) {
            return Ok(items);
        }
        let dir = self.metadata_segments(segs).await?;
        if !dir.is_dir {
            return Err(FsError::NotFound);
        }
        let mut url = format!(
            "{}/drives/{}/items/{}/children?$top={}&$select={}",
            self.ctx.base_url(),
            self.ctx.drive_id,
            dir.id,
            CHILDREN_PAGE_SIZE,
            CHILDREN_SELECT
        );
        let mut items = Vec::new();
        loop {
            let page = self
                .ctx
                .get_children_json(&url)
                .await
                .map_err(|e| fs_error(&e))?;
            items.extend(page.value.into_iter().map(raw_to_cached));
            match page.next_link {
                Some(next) => url = next,
                None => break,
            }
        }
        items.retain(|i| !is_junk_file(&i.name));
        sort_children(&mut items);
        self.ctx.cache.put_children(&rel, items.clone());
        Ok(items)
    }
}

impl DavFileSystem for GraphDavFs {
    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<'a, Box<dyn DavMetaData>> {
        Box::pin(async move {
            let segs = self.segments(path);
            let item = self.metadata_segments(&segs).await?;
            Ok(Box::new(GraphMeta(item)) as Box<dyn DavMetaData>)
        })
    }

    fn read_dir<'a>(
        &'a self,
        path: &'a DavPath,
        _meta: ReadDirMeta,
    ) -> FsFuture<'a, FsStream<Box<dyn DavDirEntry>>> {
        Box::pin(async move {
            let segs = self.segments(path);
            let items = self.children_of(&segs).await?;
            let entries: Vec<Box<dyn DavDirEntry>> = items
                .into_iter()
                .map(|item| Box::new(GraphDirEntry(item)) as Box<dyn DavDirEntry>)
                .collect();
            Ok(Box::pin(futures_util::stream::iter(entries)) as FsStream<Box<dyn DavDirEntry>>)
        })
    }

    fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<'a, Box<dyn DavFile>> {
        Box::pin(async move {
            let segs = self.segments(path);
            let name = segs.last().cloned();

            // Partial updates (PATCH-style Range PUT) are not supported:
            // Graph upload sessions are whole-file. dav-server turns this
            // into 501 so capable clients fall back to a full PUT.
            if options.write && (options.append || !options.truncate) {
                return Err(FsError::NotImplemented);
            }
            if !options.write {
                // Existing read path.
                let item = self.metadata_segments(&segs).await?;
                if item.is_dir {
                    return Err(FsError::Forbidden);
                }
                return Ok(Box::new(GraphFile {
                    ctx: Arc::clone(&self.ctx),
                    item,
                    pos: 0,
                    buf: bytes::Bytes::new(),
                    buf_start: 0,
                    download_url: None,
                }) as Box<dyn DavFile>);
            }

            // ── Write path (PUT) ──
            let Some(name) = name else {
                return Err(FsError::Forbidden); // cannot PUT the mount root
            };
            if is_junk_file(&name) {
                // Black-hole OS droppings: reply success, store nothing.
                return Ok(Box::new(GraphWriteFile {
                    ctx: Arc::clone(&self.ctx),
                    rel: segs,
                    written: 0,
                    spill: WriteSpill::Memory(bytes::BytesMut::new()),
                    junk: true,
                    pos: 0,
                }) as Box<dyn DavFile>);
            }
            // Precondition handling: If-None-Match:* (create_new) must fail
            // with Exists when the target exists; If-Match:* (!create) must
            // fail with NotFound when it doesn't. dav-server converts both
            // into 412 PRECONDITION_FAILED.
            if options.create_new || !options.create {
                let exists = match self.metadata_segments(&segs).await {
                    Ok(_) => true,
                    Err(FsError::NotFound) => false,
                    Err(e) => return Err(e),
                };
                if options.create_new && exists {
                    return Err(FsError::Exists);
                }
                if !options.create && !exists {
                    return Err(FsError::NotFound);
                }
            }
            // honor the size hint: pre-spill huge PUTs instead of growing a
            // 4 GiB BytesMut one 256 KB chunk at a time.
            let spill = if options
                .size
                .map(|s| s as usize > WRITE_SPILL_THRESHOLD)
                .unwrap_or(false)
            {
                WriteSpill::new_file()
                    .await
                    .map_err(|_| FsError::GeneralFailure)?
            } else {
                WriteSpill::Memory(bytes::BytesMut::new())
            };
            Ok(Box::new(GraphWriteFile {
                ctx: Arc::clone(&self.ctx),
                rel: segs,
                written: 0,
                spill,
                junk: false,
                pos: 0,
            }) as Box<dyn DavFile>)
        })
    }

    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<'a, ()> {
        Box::pin(async move {
            let segs = self.segments(path);
            let Some(name) = segs.last().cloned() else {
                return Err(FsError::Exists); // MKCOL on the mount root
            };
            if is_junk_file(&name) {
                return Ok(()); // black-hole, consistent with junk PUTs
            }
            let parent = self
                .metadata_segments(&segs[..segs.len() - 1])
                .await?; // NotFound -> dav-server 409 CONFLICT
            let url = format!(
                "{}/drives/{}/items/{}/children",
                self.ctx.base_url(),
                self.ctx.drive_id,
                parent.id
            );
            let item: CachedItem = match self.ctx.post_json(&url, &create_folder_body(&name)).await
            {
                Ok(json) => {
                    raw_to_cached(serde_json::from_value(json).map_err(|_| FsError::GeneralFailure)?)
                }
                Err(e) => return Err(write_fs_error(&e)), // 409 -> Exists -> 405
            };
            let rel = segs.join("/");
            self.ctx.cache.invalidate_parent(&rel);
            self.ctx.cache.put_item(&rel, item);
            Ok(())
        })
    }

    fn remove_file<'a>(&'a self, path: &'a DavPath) -> FsFuture<'a, ()> {
        Box::pin(async move {
            let segs = self.segments(path);
            if segs.is_empty() {
                return Err(FsError::Forbidden); // never delete the mount root
            }
            if write_target_is_junk(&segs) {
                return Ok(()); // invisible junk: pretend it's gone
            }
            let item = self.metadata_segments(&segs).await?;
            self.ctx
                .delete_item(&item.id)
                .await
                .map_err(|e| fs_error(&e))?;
            self.ctx.cache.invalidate_parent(&segs.join("/"));
            Ok(())
        })
    }

    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<'a, ()> {
        Box::pin(async move {
            let segs = self.segments(path);
            if segs.is_empty() {
                return Err(FsError::Forbidden); // never delete the mount root
            }
            if write_target_is_junk(&segs) {
                return Ok(()); // Finder's .TemporaryItems & friends
            }
            let item = self.metadata_segments(&segs).await?;
            self.ctx
                .delete_item(&item.id)
                .await
                .map_err(|e| fs_error(&e))?;
            self.ctx.cache.invalidate_parent(&segs.join("/"));
            self.ctx.cache.invalidate_dir(&segs.join("/"));
            Ok(())
        })
    }

    fn rename<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<'a, ()> {
        Box::pin(async move {
            let src_segs = self.segments(from);
            let dst_segs = self.segments(to);
            if src_segs.is_empty() || dst_segs.is_empty() {
                return Err(FsError::Forbidden);
            }
            let src = self.metadata_segments(&src_segs).await?; // 404 -> 409
            let dst_parent = self
                .metadata_segments(&dst_segs[..dst_segs.len() - 1])
                .await?; // NotFound -> 409
            let Some(dst_name) = dst_segs.last().cloned() else {
                return Err(FsError::Forbidden);
            };

            // Overwrite: dav-server pre-deletes destination *directories*
            // only; for files it expects rename to replace in place. Graph
            // PATCH has no conflictBehavior, so fetch the destination through
            // its path (authoritative — the listing cache hides junk) and
            // drop any occupant into the recycle bin first.
            let dst_url = graph_item_path_url(
                self.ctx.base_url(),
                &self.ctx.drive_id,
                &self.ctx.root_item_id,
                &dst_segs,
            );
            if let Ok(json) = self.ctx.get_json(&dst_url).await {
                if let Ok(occupant) =
                    serde_json::from_value::<RawItem>(json).map(raw_to_cached)
                {
                    let _ = self.ctx.delete_item(&occupant.id).await;
                }
            }

            // PATCH {name, parentReference} performs rename and/or move in
            // one call. A 409 here means the destination appeared since the
            // pre-check — drop it and retry once.
            let patch_url = format!(
                "{}/drives/{}/items/{}",
                self.ctx.base_url(),
                self.ctx.drive_id,
                src.id
            );
            let patch_body = move_patch_body(&dst_name, &dst_parent.id);
            let mut moved = self
                .ctx
                .write_request(|http, tkn| {
                    patch_item(http, tkn, &patch_url, &patch_body)
                })
                .await;
            if matches!(
                &moved,
                Err(AppError::GraphApi {
                    status_code: 409,
                    ..
                })
            ) {
                if let Ok(json) = self.ctx.get_json(&dst_url).await {
                    if let Ok(occupant) =
                        serde_json::from_value::<RawItem>(json).map(raw_to_cached)
                    {
                        let _ = self.ctx.delete_item(&occupant.id).await;
                    }
                }
                moved = self
                    .ctx
                    .write_request(|http, tkn| {
                        patch_item(http, tkn, &patch_url, &patch_body)
                    })
                    .await;
            }
            moved.map_err(|e| fs_error(&e))?;

            self.ctx.cache.invalidate_parent(&src_segs.join("/"));
            self.ctx.cache.invalidate_parent(&dst_segs.join("/"));
            Ok(())
        })
    }

    fn copy<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<'a, ()> {
        Box::pin(async move {
            let src_segs = self.segments(from);
            let dst_segs = self.segments(to);
            if src_segs.is_empty() || dst_segs.is_empty() {
                return Err(FsError::Forbidden);
            }
            if write_target_is_junk(&dst_segs) {
                return Ok(()); // black-hole
            }
            let src = self.metadata_segments(&src_segs).await?;
            let dst_parent = self
                .metadata_segments(&dst_segs[..dst_segs.len() - 1])
                .await?;
            // Overwrite: same as rename() — drop the occupant first. COPY is
            // rare enough that a raced 409 isn't worth a retry round.
            let dst_url = graph_item_path_url(
                self.ctx.base_url(),
                &self.ctx.drive_id,
                &self.ctx.root_item_id,
                &dst_segs,
            );
            if let Ok(json) = self.ctx.get_json(&dst_url).await {
                if let Ok(occupant) =
                    serde_json::from_value::<RawItem>(json).map(raw_to_cached)
                {
                    let _ = self.ctx.delete_item(&occupant.id).await;
                }
            }
            let url = format!(
                "{}/drives/{}/items/{}/copy",
                self.ctx.base_url(),
                self.ctx.drive_id,
                src.id
            );
            let Some(dst_name) = dst_segs.last().cloned() else {
                return Err(FsError::Forbidden);
            };
            self.ctx
                .write_request(|http, tkn| {
                    http.post(&url)
                        .bearer_auth(tkn)
                        .header("Content-Type", "application/json")
                        .body(copy_body(&dst_name, &dst_parent.id).to_string())
                })
                .await
                .map_err(|e| fs_error(&e))?; // 202 Accepted: async server-side
            self.ctx.cache.invalidate_parent(&dst_segs.join("/"));
            Ok(())
        })
    }
}

/// One rename/move PATCH attempt.
fn patch_item(
    http: &reqwest::Client,
    token: &str,
    url: &str,
    body: &serde_json::Value,
) -> reqwest::RequestBuilder {
    http.patch(url)
        .bearer_auth(token)
        .header("Content-Type", "application/json")
        .body(body.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Metadata / dir entry / file
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GraphMeta(CachedItem);

impl DavMetaData for GraphMeta {
    fn len(&self) -> u64 {
        self.0.size
    }

    fn modified(&self) -> FsResult<SystemTime> {
        self.0.modified.ok_or(FsError::NotImplemented)
    }

    fn created(&self) -> FsResult<SystemTime> {
        self.0.created.ok_or(FsError::NotImplemented)
    }

    fn is_dir(&self) -> bool {
        self.0.is_dir
    }

    fn etag(&self) -> Option<String> {
        self.0.etag.clone()
    }
}

struct GraphDirEntry(CachedItem);

impl DavDirEntry for GraphDirEntry {
    fn name(&self) -> Vec<u8> {
        self.0.name.clone().into_bytes()
    }

    fn metadata(&self) -> FsFuture<'_, Box<dyn DavMetaData>> {
        Box::pin(futures_util::future::ok(
            Box::new(GraphMeta(self.0.clone())) as Box<dyn DavMetaData>
        ))
    }
}

/// Read-only file backed by the Graph pre-authenticated download URL.
/// Serves dav-server's sequential `read_bytes` from a bounded prefetch
/// buffer; each buffer refill is one Range request against the download URL.
pub struct GraphFile {
    ctx: Arc<FsCtx>,
    item: CachedItem,
    pos: u64,
    buf: bytes::Bytes,
    buf_start: u64,
    download_url: Option<String>,
}

// DavFile requires Debug; FsCtx (AppHandle/GraphClient) has no Debug derive.
impl std::fmt::Debug for GraphFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphFile")
            .field("item", &self.item.name)
            .field("size", &self.item.size)
            .field("pos", &self.pos)
            .finish()
    }
}

impl GraphFile {
    /// Fetch a fresh buffer starting at `self.pos`, at least `want` bytes.
    async fn fill(&mut self, want: usize) -> FsResult<()> {
        let url = match &self.download_url {
            Some(u) => u.clone(),
            None => {
                let u = self
                    .ctx
                    .fetch_download_url(&self.item.id)
                    .await
                    .map_err(|e| {
                        log::warn!(
                            "[webdav] no download URL for {} ({}): {:?}",
                            self.item.name,
                            self.item.id,
                            e
                        );
                        FsError::GeneralFailure
                    })?;
                self.download_url = Some(u.clone());
                u
            }
        };
        let start = self.pos;
        let remaining = self.item.size - start;
        let chunk_len = std::cmp::min(want.max(PREFETCH_CHUNK) as u64, remaining);
        let end = start + chunk_len - 1; // inclusive
        let range = format!("bytes={}-{}", start, end);
        let response = self
            .ctx
            .http()
            .get(&url)
            .header("Range", &range)
            .timeout(RANGE_FETCH_TIMEOUT)
            .send()
            .await
            .map_err(|e| {
                log::warn!(
                    "[webdav] range fetch {} for {} failed: {}",
                    range,
                    self.item.name,
                    e
                );
                FsError::GeneralFailure
            })?;
        if !response.status().is_success() {
            log::warn!(
                "[webdav] range fetch {} for {} got HTTP {}",
                range,
                self.item.name,
                response.status()
            );
            return Err(FsError::GeneralFailure);
        }
        let body = response.bytes().await.map_err(|e| {
            log::warn!(
                "[webdav] range body for {} failed: {}",
                self.item.name,
                e
            );
            FsError::GeneralFailure
        })?;
        self.buf_start = start;
        self.buf = body;
        Ok(())
    }
}

impl DavFile for GraphFile {
    fn metadata(&mut self) -> FsFuture<'_, Box<dyn DavMetaData>> {
        Box::pin(futures_util::future::ok(
            Box::new(GraphMeta(self.item.clone())) as Box<dyn DavMetaData>
        ))
    }

    fn write_buf(&mut self, _buf: Box<dyn bytes::Buf + Send>) -> FsFuture<'_, ()> {
        Box::pin(futures_util::future::err(FsError::Forbidden))
    }

    fn write_bytes(&mut self, _buf: bytes::Bytes) -> FsFuture<'_, ()> {
        Box::pin(futures_util::future::err(FsError::Forbidden))
    }

    fn read_bytes(&mut self, count: usize) -> FsFuture<'_, bytes::Bytes> {
        Box::pin(async move {
            let size = self.item.size;
            if count == 0 || self.pos >= size {
                return Ok(bytes::Bytes::new());
            }
            let offset = self.pos - self.buf_start;
            if offset >= self.buf.len() as u64 {
                self.fill(count).await?;
            }
            let offset = (self.pos - self.buf_start) as usize;
            let available = self.buf.len() - offset;
            let take = std::cmp::min(
                std::cmp::min(count, available) as u64,
                size - self.pos,
            ) as usize;
            let out = self.buf.slice(offset..offset + take);
            self.pos += take as u64;
            Ok(out)
        })
    }

    fn seek(&mut self, pos: std::io::SeekFrom) -> FsFuture<'_, u64> {
        Box::pin(async move {
            let target = match pos {
                std::io::SeekFrom::Start(n) => n as i64,
                std::io::SeekFrom::End(n) => self.item.size as i64 + n,
                std::io::SeekFrom::Current(n) => self.pos as i64 + n,
            };
            if target < 0 {
                return Err(FsError::GeneralFailure);
            }
            self.pos = target as u64;
            Ok(self.pos)
        })
    }

    fn flush(&mut self) -> FsFuture<'_, ()> {
        Box::pin(futures_util::future::ok(()))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PUT: write-side file handle
// ─────────────────────────────────────────────────────────────────────────────

/// Buffered PUT body: stays in memory below the spill threshold, then moves
/// to a temp file so gigabyte Explorer copies never balloon the process.
enum WriteSpill {
    Memory(bytes::BytesMut),
    File(SpillFile),
}

struct SpillFile {
    file: tokio::fs::File,
    path: PathBuf,
    len: u64,
}

impl WriteSpill {
    async fn new_file() -> Result<Self, std::io::Error> {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "shareonelist-webdav-{}-{}.upload",
            std::process::id(),
            nanos
        ));
        // read+write: the session upload re-reads the body from this handle
        // after seeking back to 0 (a write-only File::create handle answers
        // the later read with os error 5 on Windows).
        let file = tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .await?;
        Ok(WriteSpill::File(SpillFile { file, path, len: 0 }))
    }

    fn len(&self) -> u64 {
        match self {
            WriteSpill::Memory(m) => m.len() as u64,
            WriteSpill::File(f) => f.len,
        }
    }
}

/// Write-side handle behind `open(write)`. dav-server streams the request
/// body in via `write_bytes`/`write_buf` and finishes with `flush()`, which
/// performs the actual Graph upload (simple PUT, or a resumable session for
/// large bodies). Junk-named files short-circuit: nothing is ever uploaded.
pub struct GraphWriteFile {
    ctx: Arc<FsCtx>,
    /// Mount-relative segments of the target file.
    rel: Vec<String>,
    written: u64,
    spill: WriteSpill,
    junk: bool,
    pos: u64,
}

// DavFile requires Debug; FsCtx has no Debug derive.
impl std::fmt::Debug for GraphWriteFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphWriteFile")
            .field("rel", &self.rel)
            .field("written", &self.written)
            .field("junk", &self.junk)
            .finish()
    }
}

impl GraphWriteFile {
    async fn ingest(&mut self, chunk: bytes::Bytes) -> FsResult<()> {
        self.written += chunk.len() as u64;
        match &mut self.spill {
            WriteSpill::Memory(mem) => {
                if stays_in_memory(mem.len(), chunk.len()) {
                    mem.extend_from_slice(&chunk);
                    return Ok(());
                }
                // Switch to a temp file; replay what memory already holds.
                let spilled = WriteSpill::new_file().await.map_err(|e| {
                    log::warn!("[webdav] PUT spill file create failed: {}", e);
                    FsError::GeneralFailure
                })?;
                let WriteSpill::File(mut file) = spilled else {
                    unreachable!("new_file always returns the File variant");
                };
                file.file
                    .write_all(&mem)
                    .await
                    .map_err(|e| {
                        log::warn!("[webdav] PUT spill replay failed: {}", e);
                        FsError::GeneralFailure
                    })?;
                file.file.write_all(&chunk).await.map_err(|e| {
                    log::warn!("[webdav] PUT spill write failed: {}", e);
                    FsError::GeneralFailure
                })?;
                file.len = (mem.len() + chunk.len()) as u64;
                self.spill = WriteSpill::File(file);
            }
            WriteSpill::File(f) => {
                f.file.write_all(&chunk).await.map_err(|e| {
                    log::warn!("[webdav] PUT spill write failed: {}", e);
                    FsError::GeneralFailure
                })?;
                f.len += chunk.len() as u64;
            }
        }
        Ok(())
    }

    /// Push the buffered body into OneDrive. Memory-resident bodies go out as
    /// one simple PUT (conflictBehavior=replace: WebDAV PUT means "store
    /// this"); larger bodies stream through a resumable upload session in
    /// 4 MiB chunks with per-chunk retries.
    async fn upload(&mut self) -> FsResult<()> {
        let total = self.spill.len() as usize;
        let rel_display = self.rel.join("/");
        log::info!("[webdav] PUT upload start {} ({} bytes)", rel_display, total);
        let url = write_content_url(
            self.ctx.base_url(),
            &self.ctx.drive_id,
            &self.ctx.root_item_id,
            &self.rel,
        );
        if let WriteSpill::Memory(mem) = &mut self.spill {
            let body = mem.split().freeze();
            let result = self
                .ctx
                .write_request(|http, tkn| {
                    http.put(&url)
                        .bearer_auth(tkn)
                        .header("Content-Type", "application/octet-stream")
                        .body(body.clone())
                })
                .await;
            match result {
                Ok(_) => {
                    log::info!("[webdav] PUT simple upload ok {} ({} bytes)", rel_display, total);
                    return Ok(());
                }
                Err(e) => {
                    log::warn!("[webdav] PUT simple upload FAILED {}: {:?} url={}", rel_display, e, url);
                    return Err(write_fs_error(&e));
                }
            }
        }

        // Session upload from the temp file.
        let WriteSpill::File(f) = &mut self.spill else {
            log::warn!("[webdav] PUT {}: expected spilled body, got memory", rel_display);
            unreachable!("total body is on disk here");
        };
        if let Err(e) = f.file.flush().await {
            log::warn!("[webdav] PUT {}: temp flush failed: {}", rel_display, e);
            return Err(FsError::GeneralFailure);
        }
        if let Err(e) = f.file.seek(std::io::SeekFrom::Start(0)).await {
            log::warn!("[webdav] PUT {}: temp seek failed: {}", rel_display, e);
            return Err(FsError::GeneralFailure);
        }

        let session_url = write_session_url(
            self.ctx.base_url(),
            &self.ctx.drive_id,
            &self.ctx.root_item_id,
            &self.rel,
        );
        log::info!("[webdav] PUT {}: creating upload session", rel_display);
        let session = self
            .ctx
            .post_json(&session_url, &json!({
                "item": { "@microsoft.graph.conflictBehavior": "replace" }
            }))
            .await
            .map_err(|e| {
                log::warn!(
                    "[webdav] upload session create FAILED {}: {:?} url={}",
                    rel_display,
                    e,
                    session_url
                );
                write_fs_error(&e)
            })?;
        let upload_url = session["uploadUrl"]
            .as_str()
            .ok_or_else(|| {
                log::warn!("[webdav] upload session response missing uploadUrl");
                FsError::GeneralFailure
            })?
            .to_string();

        let http = reqwest::Client::builder()
            .timeout(UPLOAD_CHUNK_TIMEOUT)
            .build()
            .map_err(|e| {
                log::warn!("[webdav] PUT {}: upload client build failed: {}", rel_display, e);
                FsError::GeneralFailure
            })?;

        let mut start = 0usize;
        while start < total {
            let end = (start + UPLOAD_CHUNK_SIZE).min(total);
            let mut chunk = vec![0u8; end - start];
            if let Err(e) = f.file.read_exact(&mut chunk).await {
                log::warn!("[webdav] PUT {}: temp read at {} failed: {}", rel_display, start, e);
                return Err(FsError::GeneralFailure);
            }
            let range = content_range(start, end, total);
            let mut ok = false;
            for attempt in 0..UPLOAD_CHUNK_RETRIES {
                let response = http
                    .put(&upload_url)
                    .header("Content-Range", &range)
                    .header("Content-Length", end - start)
                    .body(chunk.clone())
                    .send()
                    .await;
                match response {
                    Ok(r) if r.status().is_success() => {
                        ok = true;
                        break;
                    }
                    Ok(r) => {
                        log::warn!(
                            "[webdav] upload chunk {} got HTTP {} (attempt {})",
                            range,
                            r.status(),
                            attempt + 1
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "[webdav] upload chunk {} failed: {} (attempt {})",
                            range,
                            e,
                            attempt + 1
                        );
                    }
                }
                tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
            }
            if !ok {
                log::warn!("[webdav] PUT {}: chunk {} exhausted retries", rel_display, range);
                return Err(FsError::GeneralFailure);
            }
            start = end;
        }
        log::info!("[webdav] PUT session upload ok {} ({} bytes)", rel_display, total);
        Ok(())
    }
}

impl Drop for GraphWriteFile {
    fn drop(&mut self) {
        if let WriteSpill::File(f) = &self.spill {
            // Temp files are unlinked synchronously: flush() already closed
            // out its data, and an aborted PUT must not leave droppings.
            let _ = std::fs::remove_file(&f.path);
        }
    }
}

impl DavFile for GraphWriteFile {
    fn metadata(&mut self) -> FsFuture<'_, Box<dyn DavMetaData>> {
        Box::pin(futures_util::future::ok(
            Box::new(GraphMeta(CachedItem {
                id: String::new(),
                name: self.rel.last().cloned().unwrap_or_default(),
                size: self.written,
                is_dir: false,
                mime: None,
                etag: None,
                modified: None,
                created: None,
            })) as Box<dyn DavMetaData>
        ))
    }

    fn write_buf(&mut self, mut buf: Box<dyn bytes::Buf + Send>) -> FsFuture<'_, ()> {
        Box::pin(async move {
            while buf.has_remaining() {
                let chunk = buf.copy_to_bytes(buf.chunk().len());
                self.ingest(chunk).await?;
            }
            Ok(())
        })
    }

    fn write_bytes(&mut self, buf: bytes::Bytes) -> FsFuture<'_, ()> {
        Box::pin(async move { self.ingest(buf).await })
    }

    fn read_bytes(&mut self, _count: usize) -> FsFuture<'_, bytes::Bytes> {
        Box::pin(futures_util::future::err(FsError::Forbidden))
    }

    fn seek(&mut self, pos: std::io::SeekFrom) -> FsFuture<'_, u64> {
        Box::pin(async move {
            let target = match pos {
                std::io::SeekFrom::Start(n) => n as i64,
                std::io::SeekFrom::End(n) => self.written as i64 + n,
                std::io::SeekFrom::Current(n) => self.pos as i64 + n,
            };
            if target < 0 {
                return Err(FsError::GeneralFailure);
            }
            self.pos = target as u64;
            Ok(self.pos)
        })
    }

    /// Upload the buffered body and refresh the directory cache so the next
    /// listing sees the new size/entry.
    fn flush(&mut self) -> FsFuture<'_, ()> {
        Box::pin(async move {
            log::info!(
                "[webdav] PUT flush {} ({} bytes, junk={})",
                self.rel.join("/"),
                self.written,
                self.junk
            );
            if !self.junk {
                self.upload().await?;
            }
            self.ctx.cache.invalidate_parent(&self.rel.join("/"));
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use super::*;

    fn item(name: &str, is_dir: bool) -> CachedItem {
        CachedItem {
            id: format!("id-{}", name),
            name: name.to_string(),
            size: 0,
            is_dir,
            mime: None,
            etag: None,
            modified: None,
            created: None,
        }
    }

    #[test]
    fn junk_files_are_detected() {
        assert!(is_junk_file(".DS_Store"));
        assert!(is_junk_file("._notes"));
        assert!(is_junk_file("Thumbs.db"));
        assert!(is_junk_file("desktop.ini"));
        assert!(!is_junk_file("report.docx"));
        assert!(!is_junk_file(".gitignore")); // dotfiles are legitimate content
        assert!(!is_junk_file("Thumbs-up.png"));
    }

    #[test]
    fn children_sort_dirs_first_then_name() {
        let mut items = vec![
            item("b.txt", false),
            item(" Folder", true),
            item("Apple", false),
            item("zeta", true),
            item("Ápple", false),
        ];
        sort_children(&mut items);
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        // "Ápple" lowercases to "ápple" whose UTF-8 bytes sort after "b.txt"
        // (0xC3 > 0x62). Deterministic byte order is fine for listings.
        assert_eq!(names, vec![" Folder", "zeta", "Apple", "b.txt", "Ápple"]);
    }

    #[test]
    fn decode_segments_handles_root_and_nesting() {
        assert!(decode_segments(b"/").is_empty());
        assert!(decode_segments(b"").is_empty());
        assert_eq!(
            decode_segments(b"/docs/2026/report final.docx"),
            vec!["docs", "2026", "report final.docx"]
    );
    }

    // Feature: webdav-mount, Property 6: the download URL is read from the
    // plain driveItem JSON — a `$select` projection (which drops the instance
    // annotation) must never be used, and a full item must parse.
    #[test]
    fn download_url_extracts_instance_annotation() {
        let full: serde_json::Value = serde_json::json!({
            "id": "01ABC",
            "name": "a.zip",
            "size": 1234,
            "@content.downloadUrl": "https://spo.example/other",
            "@microsoft.graph.downloadUrl": "https://spo.example/preauth?tempauth=xyz",
        });
        assert_eq!(
            extract_download_url(&full).as_deref(),
            Some("https://spo.example/preauth?tempauth=xyz")
        );
        // A $select-shaped response: no annotation -> None (never panic).
        let projected: serde_json::Value = serde_json::json!({ "id": "01ABC" });
        assert!(extract_download_url(&projected).is_none());
        assert!(extract_download_url(&serde_json::Value::Null).is_none());
    }

    #[test]
    fn graph_urls_encode_specials_and_colon_syntax() {
        let base = "https://graph.microsoft.com/v1.0";
        // drive root, no segments
        assert_eq!(
            graph_item_path_url(base, "d1", "", &[]),
            format!("{}/drives/d1/root", base)
        );
        // drive root + nested path
        assert_eq!(
            graph_item_path_url(base, "d1", "", &["a b".to_string(), "c#1.txt".to_string()]),
            format!("{}/drives/d1/root:/a%20b/c%231.txt", base)
        );
        // subtree root
        assert_eq!(
            graph_item_path_url(base, "d1", "item-9", &[]),
            format!("{}/drives/d1/items/item-9", base)
        );
        assert_eq!(
            graph_item_path_url(base, "d1", "item-9", &["x".to_string()]),
            format!("{}/drives/d1/items/item-9:/x", base)
        );
    }

    // Feature: webdav-mount, Property 1: listing order (folders first, then files, name-ascending)
    proptest::proptest! {
        #[test]
        fn sorted_listing_is_always_dirs_first_name_asc(
            flags in proptest::collection::vec(proptest::bool::ANY, 0..20),
            names in proptest::collection::vec("[a-zA-Z0-9 ]{1,10}", 0..20),
        ) {
            let n = flags.len().min(names.len());
            let mut items: Vec<CachedItem> = (0..n)
                .map(|i| item(&names[i], flags[i]))
                .collect();
            sort_children(&mut items);
            let mut seen_file = false;
            let mut prev_file: Option<String> = None;
            for it in &items {
                if it.is_dir {
                    prop_assert!(!seen_file, "folder after file: {}", it.name);
                } else {
                    seen_file = true;
                    if let Some(prev) = prev_file.as_ref() {
                        prop_assert!(
                            prev.to_lowercase() <= it.name.to_lowercase(),
                            "file order violated: {} > {}", prev, it.name
                        );
                    }
                    prev_file = Some(it.name.clone());
                }
            }
        }
    }

    // Feature: webdav-mount, Property 4: junk files never appear in listings
    proptest::proptest! {
        #[test]
        fn junk_never_survives_filter(prefix in "[a-z]{0,4}", suffix in "[a-z]{0,5}") {
            // the `._` AppleDouble prefix rule
            let apple_double = format!("._{}", suffix);
            let is_ad = is_junk_file(&apple_double);
            prop_assert!(is_ad, "{} should be junk", apple_double);
            // the exact OS noise names
            prop_assert!(is_junk_file(".DS_Store"));
            prop_assert!(is_junk_file("Thumbs.db"));
            prop_assert!(is_junk_file("desktop.ini"));
            // regular files keep their names, whatever the prefix looks like
            let regular = format!("{}notes.txt", prefix);
            let not_junk = !is_junk_file(&regular);
            prop_assert!(not_junk, "{} should not be junk", regular);
        }
    }

    // ── P1 write path ──

    #[test]
    fn write_content_url_targets_content_endpoint_with_replace() {
        let url = write_content_url(
            "https://graph.microsoft.com/v1.0",
            "drive123",
            "",
            &["a b".to_string(), "报 告.docx".to_string()],
        );
        assert_eq!(
            url,
            "https://graph.microsoft.com/v1.0/drives/drive123/root:/a%20b/%E6%8A%A5%20%E5%91%8A.docx:/content?@microsoft.graph.conflictBehavior=replace"
        );
        // the conflictBehavior rides in the query string: `@` is not a legal
        // header character and reqwest refuses such requests at build time.
        assert!(url.contains(":/content?"));
    }

    #[test]
    fn write_session_url_targets_colon_syntax() {
        let url = write_session_url(
            "https://microsoftgraph.chinacloudapi.cn/v1.0",
            "drive9",
            "item42",
            &["docs".to_string(), "big.zip".to_string()],
        );
        assert_eq!(
            url,
            "https://microsoftgraph.chinacloudapi.cn/v1.0/drives/drive9/items/item42:/docs/big.zip:/createUploadSession"
        );
    }

    #[test]
    fn content_range_is_inclusive_and_total_terminated() {
        assert_eq!(content_range(0, 4_194_304, 10_000_000), "bytes 0-4194303/10000000");
        assert_eq!(content_range(4_194_304, 10_000_000, 10_000_000), "bytes 4194304-9999999/10000000");
        // degenerate total=0 never reaches the chunk loop (start < total), so
        // no underflow case exists by construction.
    }

    #[test]
    fn create_folder_body_fails_on_conflict() {
        let body = create_folder_body("新建文件夹");
        assert_eq!(body["name"], "新建文件夹");
        assert_eq!(body["@microsoft.graph.conflictBehavior"], "fail");
        assert!(body["folder"].is_object());
    }

    #[test]
    fn move_patch_body_targets_parent_and_name() {
        let body = move_patch_body("renamed.txt", "parentId7");
        assert_eq!(body["name"], "renamed.txt");
        assert_eq!(body["parentReference"]["id"], "parentId7");
        let copy = copy_body("copied.txt", "parentRef8");
        assert_eq!(copy["name"], "copied.txt");
        assert_eq!(copy["parentReference"]["id"], "parentRef8");
    }

    #[test]
    fn write_fs_error_maps_conflict_and_missing() {
        let conflict = AppError::GraphApi {
            message: "nameAlreadyExists".into(),
            status_code: 409,
        };
        assert!(matches!(write_fs_error(&conflict), FsError::Exists));
        let missing = AppError::GraphApi {
            message: "itemNotFound".into(),
            status_code: 404,
        };
        assert!(matches!(write_fs_error(&missing), FsError::NotFound));
        let other = AppError::GraphApi {
            message: "throttled".into(),
            status_code: 429,
        };
        assert!(matches!(write_fs_error(&other), FsError::GeneralFailure));
    }

    #[test]
    fn write_target_junk_decision_matches_listing_filter() {
        assert!(write_target_is_junk(&["docs".to_string(), ".DS_Store".to_string()]));
        assert!(write_target_is_junk(&["._macfile".to_string()]));
        assert!(!write_target_is_junk(&["docs".to_string(), "report.pdf".to_string()]));
        // mount root itself is never junk (it's protected by other checks)
        assert!(!write_target_is_junk(&[]));
    }

    // Feature: webdav-mount, Property 8: PUT bodies stay in memory up to the
    // spill threshold and anything beyond it moves to a temp file
    proptest::proptest! {
        #[test]
        fn spill_decision_follows_threshold(
            current in 0usize..=WRITE_SPILL_THRESHOLD,
            incoming in 1usize..=8 * 1024 * 1024,
        ) {
            let total = current + incoming;
            let in_memory = stays_in_memory(current, incoming);
            if in_memory {
                // memory-resident bodies never exceed the cap
                prop_assert!(total <= WRITE_SPILL_THRESHOLD);
            } else {
                // anything beyond the cap is destined for the temp file
                prop_assert!(total > WRITE_SPILL_THRESHOLD);
            }
        }
    }
}
