// Catalog writebacks: the three accumulation paths that grow the catalog
// from real user behavior instead of a background crawl — browsing,
// search/grounding hits (with ancestor chains), and AI file reads.

use serde_json::json;

use crate::auth::cloud_config::CloudEnvironment;
use crate::catalog::store::{CatalogNodeInput, CatalogStore};
use crate::errors::AppError;

/// One item listed while the user browsed a folder.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseItem {
    pub item_id: String,
    pub name: String,
    pub path: String, // full path relative to drive root
    pub kind: String, // 'folder' | 'file'
}

/// One search/grounding hit.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HitInput {
    pub path: String,
    #[serde(default)]
    pub item_id: String,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub desc: String,
}

fn record_usage(
    store: &CatalogStore,
    account_id: &str,
    source: &str,
    question: &str,
    paths: &[String],
) {
    let payload = json!({ "paths": paths });
    let _ = store.record_usage(account_id, source, question, &payload);
}

/// The user listed `folder_path` in the file browser: count a visit on the
/// folder itself and upsert every child (children carry fresh item ids and
/// names, so renames/deletes sync naturally — requirements 6.1).
pub fn record_browse(
    store: &CatalogStore,
    account_id: &str,
    cloud_env: &CloudEnvironment,
    drive_id: &str,
    folder_path: &str,
    folder_name: &str,
    folder_item_id: &str,
    items: &[BrowseItem],
) -> Result<(), AppError> {
    store.register_drive_if_missing(account_id, cloud_env, drive_id)?;
    let mut writes: Vec<(CatalogNodeInput, i64)> = vec![(
        CatalogNodeInput {
            path: folder_path.to_string(),
            item_id: folder_item_id.to_string(),
            name: folder_name.to_string(),
            kind: "folder".into(),
            desc: String::new(),
        },
        1,
    )];
    writes.extend(items.iter().map(|item| {
        (
            CatalogNodeInput {
                path: item.path.clone(),
                item_id: item.item_id.clone(),
                name: item.name.clone(),
                kind: item.kind.clone(),
                desc: String::new(),
            },
            0,
        )
    }));
    store.upsert_nodes_mixed(account_id, drive_id, &writes)?;
    record_usage(
        store,
        account_id,
        "browse",
        "",
        std::slice::from_ref(&folder_path.to_string()),
    );
    Ok(())
}

/// A search or grounding round returned files: upsert each hit with a visit
/// and make sure every ancestor folder exists as a queryable node
/// (ancestor item ids stay empty until the user browses there).
pub fn record_hits(
    store: &CatalogStore,
    account_id: &str,
    cloud_env: &CloudEnvironment,
    drive_id: &str,
    source: &str,
    question: &str,
    hits: &[HitInput],
) -> Result<(), AppError> {
    if hits.is_empty() {
        return Ok(());
    }
    store.register_drive_if_missing(account_id, cloud_env, drive_id)?;

    let mut writes: Vec<(CatalogNodeInput, i64)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for hit in hits {
        if seen.insert(hit.path.clone()) {
            writes.push((
                CatalogNodeInput {
                    path: hit.path.clone(),
                    item_id: hit.item_id.clone(),
                    name: hit.name.clone(),
                    kind: hit.kind.clone(),
                    desc: hit.desc.clone(),
                },
                1,
            ));
        }
        let mut path = hit.path.as_str();
        while let Some(pos) = path.rfind('/') {
            path = &path[..pos];
            if seen.insert(path.to_string()) {
                let name = path.rsplit('/').next().unwrap_or(path).to_string();
                writes.push((
                    CatalogNodeInput {
                        path: path.to_string(),
                        item_id: String::new(),
                        name,
                        kind: "folder".into(),
                        desc: String::new(),
                    },
                    1,
                ));
            }
        }
    }
    store.upsert_nodes_mixed(account_id, drive_id, &writes)?;
    let paths: Vec<String> = hits.iter().map(|h| h.path.clone()).collect();
    record_usage(store, account_id, source, question, &paths);
    Ok(())
}

/// The AI read a file's content: count the visit and remember what it was
/// about (fills an empty desc only — Property 4).
pub fn record_ai_read(
    store: &CatalogStore,
    account_id: &str,
    cloud_env: &CloudEnvironment,
    drive_id: &str,
    path: &str,
    item_id: &str,
    name: &str,
    desc: &str,
) -> Result<(), AppError> {
    store.register_drive_if_missing(account_id, cloud_env, drive_id)?;
    store.upsert_nodes_mixed(
        account_id,
        drive_id,
        &[(
            CatalogNodeInput {
                path: path.into(),
                item_id: item_id.into(),
                name: name.into(),
                kind: "file".into(),
                desc: desc.into(),
            },
            1,
        )],
    )?;
    record_usage(store, account_id, "grounding-read", "", &[path.to_string()]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use tempfile::TempDir;

    fn make_store(dir: &TempDir) -> CatalogStore {
        CatalogStore::new(dir.path().to_path_buf())
    }

    fn browse_item(path: &str, kind: &str) -> BrowseItem {
        BrowseItem {
            item_id: format!("id-{path}"),
            name: path.rsplit('/').next().unwrap_or(path).into(),
            path: path.into(),
            kind: kind.into(),
        }
    }

    #[test]
    fn browse_upserts_folder_with_children() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        record_browse(
            &store,
            "acc1",
            &CloudEnvironment::Global,
            "drv1",
            "AE&TS",
            "AE&TS",
            "id-AE&TS",
            &[browse_item("AE&TS/report.pdf", "file")],
        )
        .unwrap();

        // Auto-registered the drive and recorded both nodes.
        assert_eq!(store.list_drives().unwrap().len(), 1);
        let conn = store.open().unwrap();
        let folder_visits: i64 = conn
            .query_row(
                "SELECT visit_count FROM nodes WHERE path = 'AE&TS'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let child_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE path = 'AE&TS/report.pdf'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(folder_visits, 1);
        assert_eq!(child_count, 1);
    }

    // Feature: drive-catalog, Property 7: ancestor chains become queryable
    #[test]
    fn hit_records_complete_ancestor_chain() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        record_hits(
            &store,
            "acc1",
            &CloudEnvironment::Global,
            "drv1",
            "grounding",
            "TLOB 成本",
            &[HitInput {
                path: "AE&TS/03. TLOB/TLOB report_2025.pdf".into(),
                item_id: "file-1".into(),
                name: "TLOB report_2025.pdf".into(),
                kind: "file".into(),
                desc: "含成本与替代方案".into(),
            }],
        )
        .unwrap();

        let conn = store.open().unwrap();
        for expected in ["AE&TS", "AE&TS/03. TLOB"] {
            let (kind, count): (String, i64) = conn
                .query_row(
                    "SELECT kind, visit_count FROM nodes WHERE path = ?1",
                    params![expected],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap_or_else(|_| panic!("ancestor {expected} must exist"));
            assert_eq!(kind, "folder");
            assert_eq!(count, 1);
        }
        // Deep path search via FTS finds the folder too.
        let hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM node_fts WHERE node_fts MATCH '{name path} : \"03. TLOB\"'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(hits >= 1);
    }

    #[test]
    fn record_hits_is_idempotent_per_path() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        let hit = |desc: &str| HitInput {
            path: "R&D/notes.md".into(),
            item_id: "file-1".into(),
            name: "notes.md".into(),
            kind: "file".into(),
            desc: desc.into(),
        };
        record_hits(&store, "acc1", &CloudEnvironment::Global, "drv1", "search", "q", &[hit("d1")]).unwrap();
        record_hits(&store, "acc1", &CloudEnvironment::Global, "drv1", "search", "q", &[hit("d2")]).unwrap();

        let conn = store.open().unwrap();
        let (visits, desc): (i64, String) = conn
            .query_row(
                "SELECT visit_count, desc FROM nodes WHERE path = 'R&D/notes.md'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(visits, 2);
        assert_eq!(desc, "d1", "existing desc must not be overwritten");
    }

    // Feature: drive-catalog, Property 4: monotone visits, desc fill-only
    #[test]
    fn ai_read_fills_desc_but_never_overwrites() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        record_ai_read(&store, "acc1", &CloudEnvironment::Global, "drv1", "R&D/a.md", "f1", "a.md", "第一版描述").unwrap();
        record_ai_read(&store, "acc1", &CloudEnvironment::Global, "drv1", "R&D/a.md", "f1", "a.md", "试图覆盖").unwrap();

        let conn = store.open().unwrap();
        let (visits, desc): (i64, String) = conn
            .query_row(
                "SELECT visit_count, desc FROM nodes WHERE path = 'R&D/a.md'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(visits, 2);
        assert_eq!(desc, "第一版描述");
    }
}
