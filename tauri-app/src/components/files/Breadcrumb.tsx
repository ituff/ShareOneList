import { ChevronRight } from "lucide-react";
import { useFileStore } from "../../stores/fileStore";

/**
 * Breadcrumb navigation bar showing the path hierarchy from root to current folder.
 * Each item except the last is clickable and navigates to that ancestor folder.
 * The last item represents the current folder and is styled differently (bold, not clickable).
 */
export function Breadcrumb() {
  const breadcrumbs = useFileStore((s) => s.breadcrumbs);
  const navigateToBreadcrumb = useFileStore((s) => s.navigateToBreadcrumb);

  if (breadcrumbs.length === 0) return null;

  return (
    <nav
      aria-label="Breadcrumb"
      className="flex items-center gap-1 text-sm overflow-x-auto py-1"
    >
      <ol className="flex items-center gap-1 list-none p-0 m-0">
        {breadcrumbs.map((item, index) => {
          const isLast = index === breadcrumbs.length - 1;

          return (
            <li key={item.id} className="flex items-center gap-1">
              {index > 0 && (
                <ChevronRight
                  className="h-4 w-4 shrink-0 text-muted-foreground"
                  aria-hidden="true"
                />
              )}
              {isLast ? (
                <span
                  className="font-semibold text-foreground truncate max-w-[200px]"
                  aria-current="page"
                  title={item.name}
                >
                  {item.name}
                </span>
              ) : (
                <button
                  type="button"
                  onClick={() => navigateToBreadcrumb(index)}
                  className="text-muted-foreground hover:text-foreground hover:underline truncate max-w-[200px] transition-colors"
                  title={item.name}
                >
                  {item.name}
                </button>
              )}
            </li>
          );
        })}
      </ol>
    </nav>
  );
}
