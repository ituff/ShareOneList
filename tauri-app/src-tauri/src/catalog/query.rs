// Catalog queries: FTS5 trigram substring search with a LIKE fallback for
// short keywords, hard account filtering, and visit-weighted ordering.

use crate::auth::cloud_config::CloudEnvironment;
use crate::catalog::store::env_str;
use crate::catalog::store::CatalogStore;
use crate::errors::AppError;

/// One query result: a node plus the drive/site it belongs to.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogHit {
    pub account_id: String,
    pub cloud_env: String,
    pub drive_id: String,
    pub drive_name: String,
    pub site_name: String,
    pub path: String,
    pub item_id: String,
    pub name: String,
    pub kind: String,
    pub desc: String,
    pub visit_count: i64,
}

/// Account scope for a query: (home_account_id, cloud_env) pairs. Hits can
/// only come from drives inside this set (Property 5).
pub type AccountScope = Vec<(String, CloudEnvironment)>;

/// Wire shape of one account-scope entry (frontend input).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountKey {
    pub account_id: String,
    pub cloud_env: String,
}

/// Builds the FTS5 MATCH expression: every keyword must match in at least
/// one of name/path/desc. Trigram phrase queries act as substring matches.
fn build_match(keywords: &[String]) -> String {
    keywords
        .iter()
        .map(|kw| {
            let phrase = format!("\"{}\"", kw.replace('"', "\"\""));
            format!("(name:{phrase} OR path:{phrase} OR desc:{phrase})")
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Queries the catalog. Keywords shorter than 3 characters cannot use the
/// trigram index — when all keywords are short the query falls back to LIKE;
/// mixed queries run FTS on long keywords and post-filter on all of them.
pub fn query_catalog(
    store: &CatalogStore,
    keywords: &[String],
    accounts: Option<&AccountScope>,
    limit: usize,
) -> Result<Vec<CatalogHit>, AppError> {
    let trimmed: Vec<String> = keywords
        .iter()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let conn = store.open()?;

    let fts_keywords: Vec<String> = trimmed
        .iter()
        .filter(|k| k.chars().count() >= 3)
        .cloned()
        .collect();

    let select = "SELECT n.account_id, d.cloud_env, n.drive_id, d.name, d.site_name,
                         n.path, n.item_id, n.name, n.kind, n.desc, n.visit_count
                  FROM nodes n
                  JOIN drives d ON d.account_id = n.account_id AND d.drive_id = n.drive_id";
    let fts_join = if !fts_keywords.is_empty() {
        " JOIN node_fts f ON f.rowid = n.rowid"
    } else {
        ""
    };

    // WHERE: keyword clauses AND account scope.
    let mut where_parts: Vec<String> = Vec::new();
    let mut bind_values: Vec<String> = Vec::new();
    if !fts_keywords.is_empty() {
        where_parts.push("n.rowid IN (SELECT rowid FROM node_fts WHERE node_fts MATCH ?1)".into());
        bind_values.push(build_match(&fts_keywords));
    } else {
        for kw in &trimmed {
            where_parts.push("(n.name LIKE ? OR n.path LIKE ?)".into());
            bind_values.push(format!("%{kw}%"));
            bind_values.push(format!("%{kw}%"));
        }
    }

    match accounts {
        Some(scope) if !scope.is_empty() => {
            let clauses: Vec<String> = scope
                .iter()
                .map(|_| "(n.account_id = ? AND d.cloud_env = ?)".to_string())
                .collect();
            where_parts.push(format!("({})", clauses.join(" OR ")));
            for (account_id, env) in scope {
                bind_values.push(account_id.clone());
                bind_values.push(env_str(env).to_string());
            }
        }
        Some(_) => where_parts.push("0 = 1".into()), // empty scope: no results
        None => {}
    }

    let sql = format!(
        "{}{}{} ORDER BY n.visit_count DESC, length(n.path) ASC LIMIT {}",
        select,
        fts_join,
        if where_parts.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", where_parts.join(" AND "))
        },
        limit
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| AppError::Config {
        message: e.to_string(),
    })?;
    let rows = stmt
        .query_map(
            rusqlite::params_from_iter(bind_values.iter()),
            |row| {
                Ok(CatalogHit {
                    account_id: row.get(0)?,
                    cloud_env: row.get(1)?,
                    drive_id: row.get(2)?,
                    drive_name: row.get(3)?,
                    site_name: row.get(4)?,
                    path: row.get(5)?,
                    item_id: row.get(6)?,
                    name: row.get(7)?,
                    kind: row.get(8)?,
                    desc: row.get(9)?,
                    visit_count: row.get(10)?,
                })
            },
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
    let mut hits = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;

    // Mixed short+long keywords: FTS handled the long ones, but every
    // keyword (short included) must appear in the final hits.
    if !fts_keywords.is_empty() && trimmed.len() > fts_keywords.len() {
        hits.retain(|hit| {
            let haystack = format!("{}\n{}", hit.name, hit.path).to_lowercase();
            trimmed.iter().all(|kw| haystack.contains(&kw.to_lowercase()))
        });
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::store::{CatalogNodeInput, CatalogStore};
    use tempfile::TempDir;

    fn make_store(dir: &TempDir) -> CatalogStore {
        CatalogStore::new(dir.path().to_path_buf())
    }

    fn seed(store: &CatalogStore) {
        store
            .register_drive("acc1", &CloudEnvironment::Global, "drv1", "onedrive", "My Drive", "")
            .unwrap();
        store
            .register_drive(
                "acc2",
                &CloudEnvironment::China,
                "drv2",
                "documentLibrary",
                "Tech",
                "Tech Site",
            )
            .unwrap();
        let nodes = vec![
            CatalogNodeInput {
                path: "AE&TS/03. TLOB/TLOB report_2025.pdf".into(),
                item_id: "f1".into(),
                name: "TLOB report_2025.pdf".into(),
                kind: "file".into(),
                desc: "含成本与替代方案".into(),
            },
            CatalogNodeInput {
                path: "R&D/notes.md".into(),
                item_id: "f2".into(),
                name: "notes.md".into(),
                kind: "file".into(),
                desc: String::new(),
            },
        ];
        store.upsert_nodes("acc1", "drv1", &nodes, 0).unwrap();
        let cn_nodes = vec![CatalogNodeInput {
            path: "配方/TLOB成本.xlsx".into(),
            item_id: "f3".into(),
            name: "TLOB成本.xlsx".into(),
            kind: "file".into(),
            desc: String::new(),
        }];
        store.upsert_nodes("acc2", "drv2", &cn_nodes, 2).unwrap();
    }

    // Feature: drive-catalog, Property 6: trigram substring guarantee (CJK)
    #[test]
    fn trigram_matches_substrings_including_chinese() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        seed(&store);

        let hits = query_catalog(&store, &["TLOB".into()], None, 20).unwrap();
        assert_eq!(hits.len(), 2, "names/paths containing TLOB");

        let cn = query_catalog(&store, &["成本".into()], None, 20).unwrap();
        assert_eq!(cn.len(), 1);
        assert_eq!(cn[0].name, "TLOB成本.xlsx");

        // Path-segment substrings work too.
        let path_hits = query_catalog(&store, &["03. TLOB".into()], None, 20).unwrap();
        assert!(path_hits.iter().any(|h| h.path.contains("03. TLOB")));
    }

    // Feature: drive-catalog, Property 5: account isolation is enforced
    #[test]
    fn account_scope_filters_hits() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        seed(&store);

        let scope: AccountScope = vec![("acc1".into(), CloudEnvironment::Global)];
        let hits = query_catalog(&store, &["TLOB".into()], Some(&scope), 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].account_id, "acc1");

        // Explicitly empty scope: nothing.
        let empty: AccountScope = vec![];
        assert!(query_catalog(&store, &["TLOB".into()], Some(&empty), 20)
            .unwrap()
            .is_empty());

        // No scope: everything.
        assert_eq!(
            query_catalog(&store, &["TLOB".into()], None, 20)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn short_keywords_fall_back_to_like() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        seed(&store);
        let hits = query_catalog(&store, &["md".into()], None, 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "notes.md");
    }

    #[test]
    fn visit_count_orders_results() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        seed(&store);
        // acc2's node already has visit_count=2; bump acc1's to 5.
        store
            .upsert_nodes(
                "acc1",
                "drv1",
                &[CatalogNodeInput {
                    path: "R&D/notes.md".into(),
                    item_id: "f2".into(),
                    name: "notes.md".into(),
                    kind: "file".into(),
                    desc: String::new(),
                }],
                5,
            )
            .unwrap();
        let hits = query_catalog(&store, &["notes".into()], None, 20).unwrap();
        assert_eq!(hits[0].visit_count, 5);
    }

    #[test]
    fn empty_keywords_return_nothing() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        assert!(query_catalog(&store, &["  ".into()], None, 20)
            .unwrap()
            .is_empty());
    }
}
