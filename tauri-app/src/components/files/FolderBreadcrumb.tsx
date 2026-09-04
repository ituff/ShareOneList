import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronRight, Ellipsis, Home } from "lucide-react";
import type { BreadcrumbItem } from "../../lib/types";

interface FolderBreadcrumbProps {
  driveName: string;
  breadcrumbs: BreadcrumbItem[];
  onNavigateRoot: () => void;
  onNavigateIndex: (index: number) => void;
}

/**
 * Explorer-style breadcrumb bar. When the path is longer than the available
 * width, leading levels collapse into a "…" button whose dropdown lists the
 * hidden ancestors for direct navigation. The current folder always stays
 * visible; each remaining crumb truncates individually if still too wide.
 */
export function FolderBreadcrumb({
  driveName,
  breadcrumbs,
  onNavigateRoot,
  onNavigateIndex,
}: FolderBreadcrumbProps) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLElement>(null);
  /** How many of the middle crumbs (everything except the current folder) fit. */
  const [visibleCount, setVisibleCount] = useState(breadcrumbs.length);
  const [menuOpen, setMenuOpen] = useState(false);

  const middle = breadcrumbs.slice(0, -1);
  const current = breadcrumbs.length > 0 ? breadcrumbs[breadcrumbs.length - 1] : null;
  const hiddenCount = Math.max(middle.length - visibleCount, 0);
  const hidden = middle.slice(0, hiddenCount);
  const shown = middle.slice(hiddenCount);

  const pathKey = breadcrumbs.map((b) => b.id).join("/");

  // A new path starts fully expanded.
  useEffect(() => {
    setVisibleCount(middle.length);
    setMenuOpen(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pathKey]);

  // Collapse one leading crumb per pass while the bar overflows.
  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    if (el.scrollWidth > el.clientWidth + 1 && visibleCount > 0) {
      setVisibleCount(visibleCount - 1);
    }
  });

  // When more width becomes available, re-expand and let the shrink pass re-fit.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const observer = new ResizeObserver(() => {
      setVisibleCount(middle.length);
    });
    observer.observe(el);
    return () => observer.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [middle.length]);

  return (
    <nav
      ref={containerRef}
      className="flex min-w-0 items-center gap-1 overflow-hidden text-sm"
      aria-label="Breadcrumb"
    >
      <button
        onClick={onNavigateRoot}
        className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
      >
        <Home className="h-3.5 w-3.5 shrink-0" />
        <span className="max-w-[160px] truncate">{driveName}</span>
      </button>

      {hidden.length > 0 && (
        <span className="flex shrink-0 items-center gap-1">
          <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground" />
          <div className="relative">
            <button
              onClick={() => setMenuOpen((open) => !open)}
              className="flex items-center rounded px-1.5 py-0.5 text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
              aria-label={t("fileBrowser.breadcrumbMore")}
              aria-expanded={menuOpen}
              title={t("fileBrowser.breadcrumbMore")}
            >
              <Ellipsis className="h-3.5 w-3.5" />
            </button>
            {menuOpen && (
              <>
                <div className="fixed inset-0 z-30" onClick={() => setMenuOpen(false)} />
                <div className="absolute left-0 top-full z-40 mt-1 min-w-[160px] max-w-[280px] rounded-md border border-border bg-card p-1 shadow-lg">
                  {hidden.map((crumb, i) => (
                    <button
                      key={crumb.id}
                      onClick={() => {
                        setMenuOpen(false);
                        onNavigateIndex(i);
                      }}
                      className="block w-full truncate rounded px-2 py-1.5 text-left text-sm text-foreground hover:bg-accent transition-colors"
                      title={crumb.name}
                    >
                      {crumb.name}
                    </button>
                  ))}
                </div>
              </>
            )}
          </div>
        </span>
      )}

      {shown.map((crumb, i) => (
        <span key={crumb.id} className="flex shrink-0 items-center gap-1">
          <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground" />
          <button
            onClick={() => onNavigateIndex(hiddenCount + i)}
            className="max-w-[160px] truncate rounded px-1.5 py-0.5 text-muted-foreground hover:text-foreground hover:bg-accent transition-colors whitespace-nowrap"
            title={crumb.name}
          >
            {crumb.name}
          </button>
        </span>
      ))}

      {current && (
        <span className="flex min-w-0 items-center gap-1">
          <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground" />
          <span
            className="max-w-[240px] truncate whitespace-nowrap rounded px-1.5 py-0.5 font-semibold text-foreground"
            aria-current="page"
            title={current.name}
          >
            {current.name}
          </span>
        </span>
      )}
    </nav>
  );
}
