import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  CatalogDrive,
  CatalogNode,
  CatalogUsageSummary,
  CloudEnvironment,
} from "../../lib/types";
import {
  catalogCancelIndex,
  catalogQuery,
  catalogReindex,
  catalogStatus,
  catalogTree,
  catalogUnregisterDrive,
  catalogUsageRecent,
  formatAppError,
} from "../../lib/tauri";
import { useAuthStore } from "../../stores/authStore";
import { driveProgress, useCatalogStore } from "../../stores/catalogStore";
import { useToastStore } from "../../stores/toastStore";

/** Heat class for a node's visit count (requirements 1.2a). */
function heatClass(visitCount: number): string {
  if (visitCount >= 5) return "bg-primary/20";
  if (visitCount >= 1) return "bg-primary/8";
  return "";
}

/** One lazy-loadable tree row. */
function TreeRow({
  node,
  depth,
  accountId,
  driveId,
  childCount,
}: {
  node: CatalogNode;
  depth: number;
  accountId: string;
  driveId: string;
  childCount: number;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(depth < 2);
  const [children, setChildren] = useState<CatalogNode[] | null>(
    node.kind === "folder" && depth < 2 ? [] : null
  );
  const [loading, setLoading] = useState(false);

  const loadChildren = useCallback(async () => {
    setLoading(true);
    try {
      setChildren(await catalogTree(accountId, driveId, node.path));
    } catch {
      // The catalog is auxiliary — a failed branch renders as empty.
      setChildren([]);
    } finally {
      setLoading(false);
    }
  }, [accountId, driveId, node.path]);

  useEffect(() => {
    if (open && node.kind === "folder" && children === null) {
      loadChildren();
    }
  }, [open, node.kind, children, loadChildren]);

  const hasChildren = node.kind === "folder";
  const badge =
    node.visitCount > 0 ? (
      <span className="shrink-0 text-[10px] text-primary">🔥×{node.visitCount}</span>
    ) : null;

  return (
    <div>
      <div
        className={`flex items-center gap-1 rounded px-1 py-0.5 ${heatClass(node.visitCount)}`}
        style={{ paddingLeft: depth * 14 + 4 }}
      >
        {hasChildren ? (
          <button
            onClick={() => setOpen((o) => !o)}
            className="w-4 shrink-0 text-left text-[10px] text-muted-foreground hover:text-foreground"
            aria-label={open ? t("sitemap.collapse") : t("sitemap.expand")}
          >
            {open ? "▾" : "▸"}
          </button>
        ) : (
          <span className="w-4 shrink-0" />
        )}
        <span className="min-w-0 flex-1 truncate text-sm text-foreground" title={node.path}>
          {node.kind === "folder" ? "📁" : "📄"} {node.name}
        </span>
        {hasChildren && childCount > 0 && (
          <span className="shrink-0 text-[10px] text-muted-foreground">{childCount}</span>
        )}
        {badge}
      </div>
      {open && hasChildren && (
        <div>
          {loading && (
            <p className="py-0.5 text-xs text-muted-foreground" style={{ paddingLeft: (depth + 1) * 14 + 4 }}>
              …
            </p>
          )}
          {children?.map((child) => (
            <TreeRow
              key={child.path}
              node={child}
              depth={depth + 1}
              accountId={accountId}
              driveId={driveId}
              childCount={0}
            />
          ))}
          {children?.length === 0 && (
            <p className="py-0.5 text-xs text-muted-foreground" style={{ paddingLeft: (depth + 1) * 14 + 4 }}>
              {t("sitemap.emptyFolder")}
            </p>
          )}
        </div>
      )}
    </div>
  );

}

/** Sitemap page: registered drives, accumulation summary, tree + search. */
export function SitemapPage() {
  const { t } = useTranslation();
  const accounts = useAuthStore((s) => s.accounts);
  const addToast = useToastStore((s) => s.addToast);
  const progressMap = useCatalogStore((s) => s.progress);
  const subscribe = useCatalogStore((s) => s.subscribe);

  const [drives, setDrives] = useState<CatalogDrive[]>([]);
  const [selectedKey, setSelectedKey] = useState("");
  const [usage, setUsage] = useState<CatalogUsageSummary | null>(null);
  const [mode, setMode] = useState<"tree" | "search">("tree");
  const [searchText, setSearchText] = useState("");
  const [searchResults, setSearchResults] = useState<CatalogNode[] | null>(null);
  const [rootNodes, setRootNodes] = useState<CatalogNode[] | null>(null);

  const refresh = useCallback(async () => {
    try {
      const list = await catalogStatus();
      // Hide drives belonging to logged-out accounts.
      const ids = new Set(accounts.map((a) => a.homeAccountId));
      const visible = list.filter((d) => ids.has(d.accountId));
      setDrives(visible);
      setUsage(await catalogUsageRecent(null));
    } catch (e) {
      addToast("error", formatAppError(e));
    }
  }, [accounts, addToast]);

  useEffect(() => {
    refresh();
    const unlisten = subscribe();
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [refresh, subscribe]);

  const selected = drives.find(
    (d) => `${d.accountId}|${d.driveId}` === selectedKey
  );
  const selectedProgress = selected
    ? driveProgress(progressMap, selected.accountId, selected.driveId)
    : undefined;

  // Load the tree root when a drive is selected.
  useEffect(() => {
    if (!selected) {
      setRootNodes(null);
      return;
    }
    setRootNodes(null);
    catalogTree(selected.accountId, selected.driveId, "")
      .then(setRootNodes)
      .catch(() => setRootNodes([]));
  }, [selected]);

  const runSearch = () => {
    const keyword = searchText.trim();
    if (!keyword) {
      setSearchResults(null);
      return;
    }
    catalogQuery([keyword], null, 50)
      .then((hits) =>
        setSearchResults(
          hits.map((h) => ({
            itemId: h.itemId,
            path: h.path,
            name: h.name,
            kind: h.kind,
            desc: h.desc,
            visitCount: h.visitCount,
            lastVisited: 0,
          }))
        )
      )
      .catch((e) => addToast("error", formatAppError(e)));
  };

  const statusBadge = (status: string) => {
    const colors: Record<string, string> = {
      ready: "bg-primary/15 text-primary",
      seeding: "bg-muted text-muted-foreground",
      indexing: "bg-muted text-muted-foreground",
      failed: "bg-destructive/15 text-destructive",
      cancelled: "bg-muted text-muted-foreground",
    };
    return (
      <span className={`rounded px-1.5 py-0.5 text-[10px] ${colors[status] ?? "bg-muted text-muted-foreground"}`}>
        {t(`sitemap.status.${status}`, { defaultValue: status })}
      </span>
    );
  };

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="pb-2">
        <h2 className="text-lg font-semibold text-foreground">{t("sitemap.title")}</h2>
        <p className="text-xs text-muted-foreground">{t("sitemap.description")}</p>
      </div>

      {usage && (
        <div className="flex flex-wrap items-center gap-2 pb-2 text-xs text-muted-foreground">
          <span className="rounded bg-muted/50 px-2 py-0.5">
            {t("sitemap.summaryBrowse", { count: usage.browse })}
          </span>
          <span className="rounded bg-muted/50 px-2 py-0.5">
            {t("sitemap.summarySearch", { count: usage.search + usage.grounding })}
          </span>
          <span className="rounded bg-muted/50 px-2 py-0.5">
            {t("sitemap.summaryRead", { count: usage.groundingRead })}
          </span>
        </div>
      )}

      <div className="flex min-h-0 flex-1 gap-3">
        {/* Drive list */}
        <div className="w-64 shrink-0 space-y-1.5 overflow-auto">
          {drives.length === 0 && (
            <p className="rounded-md bg-muted/40 p-3 text-xs text-muted-foreground">
              {t("sitemap.noDrives")}
            </p>
          )}
          {drives.map((drive) => {
            const key = `${drive.accountId}|${drive.driveId}`;
            const progress = driveProgress(progressMap, drive.accountId, drive.driveId);
            return (
              <button
                key={key}
                onClick={() => setSelectedKey(key)}
                className={`w-full rounded-md border p-2 text-left ${
                  selectedKey === key
                    ? "border-primary bg-primary/5"
                    : "border-border hover:bg-accent"
                }`}
              >
                <div className="flex items-center justify-between gap-1">
                  <span className="min-w-0 flex-1 truncate text-sm font-medium text-foreground">
                    {drive.siteName || drive.name}
                  </span>
                  {statusBadge(progress?.status ?? drive.status)}
                </div>
                <p className="truncate text-xs text-muted-foreground">
                  {drive.accountId} · {t("sitemap.nodeCount", { count: drive.nodeCount })}
                </p>
                {progress?.status === "indexing" && (
                  <div className="mt-1">
                    <div className="h-1 w-full overflow-hidden rounded bg-muted">
                      <div className="h-full animate-pulse bg-primary" style={{ width: "60%" }} />
                    </div>
                    <p className="truncate text-[10px] text-muted-foreground">
                      {progress.currentPath}
                    </p>
                  </div>
                )}
              </button>
            );
          })}
        </div>

        {/* Main panel */}
        <div className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-md border border-border">
          <div className="flex items-center justify-between gap-2 border-b border-border p-2">
            <div className="flex gap-1">
              <button
                onClick={() => setMode("tree")}
                className={`rounded px-2 py-1 text-xs ${
                  mode === "tree" ? "bg-accent text-foreground" : "text-muted-foreground"
                }`}
              >
                {t("sitemap.treeMode")}
              </button>
              <button
                onClick={() => setMode("search")}
                className={`rounded px-2 py-1 text-xs ${
                  mode === "search" ? "bg-accent text-foreground" : "text-muted-foreground"
                }`}
              >
                {t("sitemap.searchMode")}
              </button>
            </div>
            {selected && (
              <div className="flex gap-1">
                {selectedProgress?.status === "indexing" ? (
                  <button
                    onClick={() =>
                      catalogCancelIndex(selected.accountId, selected.driveId).catch(() => undefined)
                    }
                    className="rounded px-2 py-1 text-xs text-foreground hover:bg-accent"
                  >
                    {t("sitemap.cancelIndex")}
                  </button>
                ) : (
                  <button
                    onClick={() => {
                      catalogReindex(selected.accountId, selected.cloudEnv as CloudEnvironment, selected.driveId, 5)
                        .then(() => refresh())
                        .catch((e) => addToast("error", formatAppError(e)));
                    }}
                    className="rounded px-2 py-1 text-xs text-foreground hover:bg-accent"
                  >
                    {t("sitemap.deepIndex")}
                  </button>
                )}
                <button
                  onClick={() => {
                    if (!window.confirm(t("sitemap.confirmRemove", { name: selected.siteName || selected.name }))) return;
                    catalogUnregisterDrive(selected.accountId, selected.driveId)
                      .then(() => {
                        setSelectedKey("");
                        return refresh();
                      })
                      .catch((e) => addToast("error", formatAppError(e)));
                  }}
                  className="rounded px-2 py-1 text-xs text-destructive hover:bg-destructive/10"
                >
                  {t("sitemap.unregister")}
                </button>
              </div>
            )}
          </div>

          <div className="min-h-0 flex-1 overflow-auto p-2">
            {!selected && (
              <p className="p-2 text-sm text-muted-foreground">{t("sitemap.selectDrive")}</p>
            )}

            {selected && mode === "tree" && (
              <>
                {rootNodes === null && (
                  <p className="p-1 text-xs text-muted-foreground">{t("sitemap.loading")}</p>
                )}
                {rootNodes?.length === 0 && (
                  <p className="p-1 text-xs text-muted-foreground">{t("sitemap.emptyFolder")}</p>
                )}
                {rootNodes?.map((node) => (
                  <TreeRow
                    key={node.path}
                    node={node}
                    depth={0}
                    accountId={selected.accountId}
                    driveId={selected.driveId}
                    childCount={0}
                  />
                ))}
              </>
            )}

            {selected && mode === "search" && (
              <>
                <div className="flex gap-1.5 pb-2">
                  <input
                    type="text"
                    value={searchText}
                    onChange={(e) => setSearchText(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && runSearch()}
                    placeholder={t("sitemap.searchPlaceholder")}
                    className="flex-1 rounded-md border border-border bg-background px-2 py-1.5 text-sm text-foreground"
                  />
                  <button
                    onClick={runSearch}
                    className="rounded-md bg-primary px-3 py-1.5 text-sm text-primary-foreground hover:bg-primary/90"
                  >
                    {t("home.searchAction")}
                  </button>
                </div>
                {searchResults?.length === 0 && (
                  <p className="p-1 text-xs text-muted-foreground">{t("home.noResults")}</p>
                )}
                {searchResults?.map((node) => (
                  <div
                    key={node.path}
                    className={`flex items-center gap-2 rounded px-1 py-0.5 ${heatClass(node.visitCount)}`}
                  >
                    <span className="min-w-0 flex-1 truncate text-sm text-foreground">
                      {node.kind === "folder" ? "📁" : "📄"} {node.path}
                    </span>
                    {node.visitCount > 0 && (
                      <span className="shrink-0 text-[10px] text-primary">🔥×{node.visitCount}</span>
                    )}
                  </div>
                ))}
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
