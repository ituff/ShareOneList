// Path -> Graph item metadata cache for the WebDAV gateway.
//
// Explorer/Finder issue far more PROPFIND/stat requests than the app UI does
// (icon resolution, double stat on every entry). This cache absorbs the storm.
// It is lazily consistent by design: entries expire after a short TTL and
// write paths must invalidate the parent listing (see invalidate_parent).

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// Entries older than this are treated as absent.
pub const CACHE_TTL: Duration = Duration::from_secs(30);
/// LRU cap over cache slots (a directory listing counts as one slot).
const MAX_ENTRIES: usize = 5000;

/// A single Graph drive item, reduced to what WebDAV metadata needs.
#[derive(Debug, Clone)]
pub struct CachedItem {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub mime: Option<String>,
    pub etag: Option<String>,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
}

struct Inner {
    /// Item metadata keyed by canonical relative path.
    items: HashMap<String, (CachedItem, Instant)>,
    /// Directory listings (filtered + sorted) keyed by dir path. Stored
    /// separately from `items` so listing a dir never evicts the dir's own
    /// stat metadata (Explorer stats the dir right after listing it).
    children: HashMap<String, (Vec<CachedItem>, Instant)>,
    lru: VecDeque<String>,
}

impl Inner {
    fn touch(&mut self, key: &str) {
        self.lru.retain(|k| k != key);
        self.lru.push_back(key.to_string());
    }

    fn evict_while_over_cap(&mut self) {
        while self.items.len() + self.children.len() > MAX_ENTRIES {
            match self.lru.pop_front() {
                Some(key) => {
                    self.items.remove(&key);
                    self.children.remove(&key);
                }
                None => break,
            }
        }
    }

    fn fresh_item(&mut self, key: &str) -> Option<CachedItem> {
        let (item, at) = self.items.get(key)?;
        if at.elapsed() > CACHE_TTL {
            return None;
        }
        let item = item.clone();
        self.touch(key);
        Some(item)
    }

    fn fresh_children(&mut self, key: &str) -> Option<Vec<CachedItem>> {
        let (items, at) = self.children.get(key)?;
        if at.elapsed() > CACHE_TTL {
            return None;
        }
        let items = items.clone();
        self.touch(key);
        Some(items)
    }
}

/// Thread-safe path-keyed cache. Keys are canonical relative paths:
/// `""` is the mount root, `"a/b"` a nested dir; never a leading slash.
pub struct PathCache {
    inner: Mutex<Inner>,
}

impl PathCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                items: HashMap::new(),
                children: HashMap::new(),
                lru: VecDeque::new(),
            }),
        }
    }

    pub fn get_item(&self, path: &str) -> Option<CachedItem> {
        let mut inner = self.inner.lock().ok()?;
        inner.fresh_item(path)
    }

    pub fn get_children(&self, dir: &str) -> Option<Vec<CachedItem>> {
        let mut inner = self.inner.lock().ok()?;
        inner.fresh_children(dir)
    }

    pub fn put_item(&self, path: &str, item: CachedItem) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.touch(path);
            inner
                .items
                .insert(path.to_string(), (item, Instant::now()));
            inner.evict_while_over_cap();
        }
    }

    pub fn put_children(&self, dir: &str, items: Vec<CachedItem>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.touch(dir);
            inner
                .children
                .insert(dir.to_string(), (items, Instant::now()));
            inner.evict_while_over_cap();
        }
    }

    /// Drop every slot for `dir` (listing + item metadata).
    pub fn invalidate_dir(&self, dir: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.items.remove(dir);
            inner.children.remove(dir);
            inner.lru.retain(|k| k != dir);
        }
    }

    /// After a write under `path`: the stale entry for `path` itself and the
    /// parent's listing must go. The parent's own item metadata stays —
    /// Graph does not bump a folder's metadata when its children change.
    pub fn invalidate_parent(&self, path: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.items.remove(path);
            inner.children.remove(path);
            inner.lru.retain(|k| k != path);
            let parent = match path.rfind('/') {
                Some(i) => &path[..i],
                None => "",
            };
            inner.children.remove(parent);
        }
    }
}

impl Default for PathCache {
    fn default() -> Self {
        Self::new()
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
    fn put_and_get_item_round_trip() {
        let cache = PathCache::new();
        cache.put_item("a/b.txt", item("b.txt", false));
        assert_eq!(cache.get_item("a/b.txt").unwrap().name, "b.txt");
        assert!(cache.get_item("a/other.txt").is_none());
    }

    #[test]
    fn listing_does_not_evict_dir_metadata() {
        let cache = PathCache::new();
        cache.put_item("dir", item("dir", true));
        cache.put_children("dir", vec![item("x.txt", false)]);
        assert!(cache.get_item("dir").is_some());
        assert!(cache.get_children("dir").is_some());
    }

    #[test]
    fn invalidate_parent_drops_listing_and_entry() {
        let cache = PathCache::new();
        cache.put_item("dir", item("dir", true));
        cache.put_children("dir", vec![item("x.txt", false)]);
        cache.put_item("dir/x.txt", item("x.txt", false));

        cache.invalidate_parent("dir/x.txt");

        // the written item and its parent listing are gone…
        assert!(cache.get_item("dir/x.txt").is_none());
        assert!(cache.get_children("dir").is_none());
        // …but the parent's own stat metadata survives.
        assert!(cache.get_item("dir").is_some());
    }

    // Feature: webdav-mount, Property 3: cache consistency after invalidation
    proptest::proptest! {
        #[test]
        fn written_paths_never_read_stale(paths in proptest::collection::vec("[a-z]{1,8}/[a-z]{1,8}", 1..20)) {
            let cache = PathCache::new();
            for path in &paths {
                let parent = match path.rfind('/') {
                    Some(i) => &path[..i],
                    None => "",
                };
                cache.put_item(path, item("new", false));
                cache.put_children(parent, vec![item("new", false)]);
                cache.invalidate_parent(path);
                prop_assert!(cache.get_item(path).is_none());
                prop_assert!(cache.get_children(parent).is_none());
            }
        }
    }
}
