import { useTranslation } from "react-i18next";
import { Search, X } from "lucide-react";
import type { SearchScope } from "../../lib/types";

interface SearchBarProps {
  query: string;
  scope: SearchScope;
  onQueryChange: (query: string) => void;
  onScopeChange: (scope: SearchScope) => void;
  onClear: () => void;
}

export function SearchBar({ query, scope, onQueryChange, onScopeChange, onClear }: SearchBarProps) {
  const { t } = useTranslation();

  return (
    <div className="flex items-center gap-2">
      {/* Search input */}
      <div className="relative flex-1 max-w-xs">
        <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground" />
        <input
          type="text"
          value={query}
          onChange={(e) => onQueryChange(e.target.value)}
          placeholder={t("search.placeholder")}
          className="h-8 w-full rounded-md border border-border bg-background pl-8 pr-8 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
          aria-label={t("search.placeholder")}
        />
        {query && (
          <button
            onClick={onClear}
            className="absolute right-2 top-1/2 -translate-y-1/2 rounded-sm p-0.5 text-muted-foreground hover:text-foreground transition-colors"
            aria-label={t("dialogs.close")}
          >
            <X className="h-3.5 w-3.5" />
          </button>
        )}
      </div>

      {/* Scope toggle */}
      <div className="flex items-center gap-0.5 rounded-md border border-border p-0.5">
        <button
          onClick={() => onScopeChange("local")}
          className={`rounded px-2 py-1 text-xs font-medium transition-colors ${
            scope === "local"
              ? "bg-primary text-primary-foreground"
              : "text-muted-foreground hover:bg-accent"
          }`}
          title={t("search.modeLocalDesc")}
          aria-label={t("search.modeLocal")}
        >
          {t("search.modeLocal")}
        </button>
        <button
          onClick={() => onScopeChange("global")}
          className={`rounded px-2 py-1 text-xs font-medium transition-colors ${
            scope === "global"
              ? "bg-primary text-primary-foreground"
              : "text-muted-foreground hover:bg-accent"
          }`}
          title={t("search.modeGlobalDesc")}
          aria-label={t("search.modeGlobal")}
        >
          {t("search.modeGlobal")}
        </button>
      </div>
    </div>
  );
}
