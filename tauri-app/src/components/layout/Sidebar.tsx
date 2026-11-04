import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  House,
  MessageCircle,
  Search,
  FolderOpen,
  Bookmark,
  Download,
  Wrench,
  Settings,
  PanelLeftClose,
  PanelLeftOpen,
  type LucideIcon,
} from "lucide-react";
import {
  useNavigationStore,
  type NavigationSection,
} from "../../stores/navigationStore";

interface NavItem {
  id: NavigationSection;
  labelKey: string;
  icon: LucideIcon;
}

const navItems: NavItem[] = [
  { id: "home", labelKey: "nav.home", icon: House },
  { id: "askai", labelKey: "nav.askai", icon: MessageCircle },
  { id: "search", labelKey: "nav.search", icon: Search },
  { id: "files", labelKey: "nav.files", icon: FolderOpen },
  { id: "bookmarks", labelKey: "nav.bookmarks", icon: Bookmark },
  { id: "tasks", labelKey: "nav.taskManager", icon: Download },
  { id: "tools", labelKey: "nav.tools", icon: Wrench },
  { id: "settings", labelKey: "nav.settings", icon: Settings },
];

const COLLAPSED_KEY = "sidebar-collapsed";

function readCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

export function Sidebar() {
  const { t } = useTranslation();
  const { activeSection, setActiveSection } = useNavigationStore();
  const [collapsed, setCollapsed] = useState(readCollapsed);

  const toggleCollapsed = () => {
    setCollapsed((value) => {
      const next = !value;
      try {
        localStorage.setItem(COLLAPSED_KEY, next ? "1" : "0");
      } catch {
        // localStorage unavailable: still collapse for this session.
      }
      return next;
    });
  };

  return collapsed ? (
    <aside className="flex w-14 flex-col items-center border-r border-border bg-card py-3">
      <button
        onClick={toggleCollapsed}
        className="mb-4 rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
        aria-label={t("nav.expand")}
        title={t("nav.expand")}
      >
        <PanelLeftOpen className="h-4 w-4" />
      </button>
      <nav className="flex flex-1 flex-col items-center gap-1">
        {navItems.map((item) => {
          const isActive = activeSection === item.id;
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              onClick={() => setActiveSection(item.id)}
              className={`flex w-10 justify-center rounded-md p-2.5 transition-colors duration-150 cursor-pointer ${
                isActive
                  ? "bg-accent text-accent-foreground"
                  : "text-muted-foreground hover:bg-accent/50 hover:text-foreground"
              }`}
              aria-current={isActive ? "page" : undefined}
              aria-label={t(item.labelKey)}
              title={t(item.labelKey)}
            >
              <Icon className="h-4 w-4 shrink-0" />
            </button>
          );
        })}
      </nav>
    </aside>
  ) : (
    <aside className="flex w-56 flex-col border-r border-border bg-card">
      <div className="flex items-center justify-between px-4 py-4">
        <h1 className="text-lg font-semibold text-foreground">ShareOneList</h1>
        <button
          onClick={toggleCollapsed}
          className="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
          aria-label={t("nav.collapse")}
          title={t("nav.collapse")}
        >
          <PanelLeftClose className="h-4 w-4" />
        </button>
      </div>
      <nav className="flex-1 px-2 space-y-1">
        {navItems.map((item) => {
          const isActive = activeSection === item.id;
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              onClick={() => setActiveSection(item.id)}
              className={`
                flex items-center gap-3 w-full px-3 py-2 rounded-md text-sm font-medium
                transition-colors duration-150 cursor-pointer
                ${
                  isActive
                    ? "bg-accent text-accent-foreground"
                    : "text-muted-foreground hover:bg-accent/50 hover:text-foreground"
                }
              `}
              aria-current={isActive ? "page" : undefined}
            >
              <Icon className="h-4 w-4 shrink-0" />
              <span>{t(item.labelKey)}</span>
            </button>
          );
        })}
      </nav>
    </aside>
  );
}
