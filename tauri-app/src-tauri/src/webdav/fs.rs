// GraphDavFileSystem: translates WebDAV filesystem operations into
// Microsoft Graph calls for one mount point.
//
// Read path only (P0): metadata / read_dir / open-for-read. Write methods
// fall through to dav-server's default FsError::NotImplemented until P1;
// `open` explicitly rejects write flags so PUT gets a clean 403.
//
// Token handling reuses AuthModule::get_token_for_account so mounts stay
// isolated per (cloud_env, home_account_id) and refresh silently. HTTP calls
// reuse GraphClient's 5xx retry. Graph access tokens never leave this process.

use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use tauri::Manager;

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
    async fn fetch_download_url(&self, item_id: &str) -> Result<String, AppError> {
        let url = format!(
            "{}/drives/{}/items/{}?$select=id,@microsoft.graph.downloadUrl",
            self.base_url(),
            self.drive_id,
            item_id
        );
        let json = self.get_json(&url).await?;
        json["@microsoft.graph.downloadUrl"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| AppError::GraphApi {
                message: "Graph did not return a download URL".to_string(),
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
            // P0 is read-only: reject anything that smells like a write.
            if options.write
                || options.append
                || options.truncate
                || options.create
                || options.create_new
                || !options.read
            {
                return Err(FsError::Forbidden);
            }
            let segs = self.segments(path);
            let item = self.metadata_segments(&segs).await?;
            if item.is_dir {
                return Err(FsError::Forbidden);
            }
            Ok(Box::new(GraphFile {
                ctx: Arc::clone(&self.ctx),
                item,
                pos: 0,
                buf: bytes::Bytes::new(),
                buf_start: 0,
                download_url: None,
            }) as Box<dyn DavFile>)
        })
    }
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
                    .map_err(|_| FsError::GeneralFailure)?;
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
            .send()
            .await
            .map_err(|_| FsError::GeneralFailure)?;
        if !response.status().is_success() {
            return Err(FsError::GeneralFailure);
        }
        let body = response.bytes().await.map_err(|_| FsError::GeneralFailure)?;
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
}
