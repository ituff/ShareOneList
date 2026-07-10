import { useTranslation } from "react-i18next";
import {
  House,
  FolderOpen,
  Download,
  Wrench,
  Settings,
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
  { id: "files", labelKey: "nav.files", icon: FolderOpen },
  { id: "tasks", labelKey: "nav.taskManager", icon: Download },
  { id: "tools", labelKey: "nav.tools", icon: Wrench },
  { id: "settings", labelKey: "nav.settings", icon: Settings },
];

export function Sidebar() {
  const { activeSection, setActiveSection } = useNavigationStore();
  const { t } = useTranslation();

  return (
    <aside className="flex flex-col w-56 h-full border-r border-border bg-card">
      <div className="px-4 py-4">
        <h1 className="text-lg font-semibold text-foreground">ShareOneList</h1>
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
