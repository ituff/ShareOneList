import { useTranslation } from "react-i18next";
import { Plus, X } from "lucide-react";
import { useTabStore } from "../../stores/tabStore";

interface TabBarProps {
  /** Open the drive selection flow to create another tab. */
  onNewTab: () => void;
  /** Called before switching to a tab (allows exiting the new-tab flow). */
  onTabSelect?: (tabId: string) => void;
  /** Called before closing a tab. */
  onCloseTab?: (tabId: string) => void;
}

/**
 * Horizontal tab bar for multi-drive file browsing.
 * Each tab shows the drive name and a close button.
 * The active tab is visually highlighted.
 */
export function TabBar({ onNewTab, onTabSelect, onCloseTab }: TabBarProps) {
  const { t } = useTranslation();
  const tabs = useTabStore((s) => s.tabs);
  const activeTabId = useTabStore((s) => s.activeTabId);
  const switchTab = useTabStore((s) => s.switchTab);
  const closeTab = useTabStore((s) => s.closeTab);

  if (tabs.length === 0) return null;

  return (
    <div
      className="flex items-center gap-1 border-b border-border px-2 py-1 overflow-x-auto"
      role="tablist"
      aria-label={t("tabs.label")}
    >
      {tabs.map((tab) => {
        const isActive = tab.id === activeTabId;
        return (
          <div
            key={tab.id}
            role="tab"
            aria-selected={isActive}
            tabIndex={isActive ? 0 : -1}
            onClick={() => {
              onTabSelect?.(tab.id);
              switchTab(tab.id);
            }}
            className={`group flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm cursor-pointer select-none transition-colors ${
              isActive
                ? "bg-primary text-primary-foreground"
                : "text-muted-foreground hover:bg-accent hover:text-foreground"
            }`}
          >
            <span className="truncate max-w-[120px]" title={tab.driveName}>
              {tab.driveName}
            </span>
            <button
              onClick={(e) => {
                e.stopPropagation();
                onCloseTab?.(tab.id);
                closeTab(tab.id);
              }}
              className={`rounded p-0.5 transition-colors ${
                isActive
                  ? "hover:bg-primary-foreground/20 text-primary-foreground"
                  : "hover:bg-muted-foreground/20 text-muted-foreground opacity-0 group-hover:opacity-100"
              }`}
              aria-label={t("tabs.close")}
              title={t("tabs.close")}
            >
              <X className="h-3 w-3" />
            </button>
          </div>
        );
      })}
      <button
        onClick={onNewTab}
        className="ml-1 rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
        aria-label={t("tabs.newTab")}
        title={t("tabs.newTab")}
      >
        <Plus className="h-4 w-4" />
      </button>
    </div>
  );
}
