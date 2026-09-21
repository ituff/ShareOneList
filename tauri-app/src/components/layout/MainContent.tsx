import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { useNavigationStore } from "../../stores/navigationStore";
import { AccountList } from "../accounts/AccountList";
import { BookmarksPage } from "../bookmarks/BookmarksPage";
import { FileBrowser } from "../files/FileBrowser";
import { PreviewPage } from "../files/PreviewPage";
import { DriveHubPage } from "../files/DriveHubPage";
import { SharePointSites } from "../files/SharePointSites";
import { DriveList } from "../files/DriveList";
import { TabBar } from "./TabBar";
import { useTabStore } from "../../stores/tabStore";
import { useSettingsStore } from "../../stores/settingsStore";
import { TaskManager } from "../tasks/TaskManager";
import { ToolsPage as ToolsPageComponent } from "../tools/ToolsPage";
import { UpdateChecker } from "../tools/UpdateChecker";
import type { AccountEntry, CloudEnvironment, DriveItem, Site } from "../../lib/types";

function HomePage() {
  const { t } = useTranslation();
  return (
    <div className="space-y-2">
      <h2 className="text-2xl font-bold text-foreground">{t("home.title")}</h2>
      <p className="text-muted-foreground">{t("home.welcome")}</p>
    </div>
  );
}

/** Navigation steps within the files page (before a tab is opened). */
type FilesNavState =
  | { step: "accounts" }
  | { step: "hub"; account: AccountEntry }
  | { step: "sharepoint-sites"; account: AccountEntry }
  | { step: "drive-list"; account: AccountEntry; mode: "sharepoint" | "shared"; site?: Site };

function FilesPage() {
  const tabs = useTabStore((s) => s.tabs);
  const activeTabId = useTabStore((s) => s.activeTabId);
  const openTab = useTabStore((s) => s.openTab);
  const openPreviewTab = useTabStore((s) => s.openPreviewTab);
  const closeTab = useTabStore((s) => s.closeTab);
  const switchTab = useTabStore((s) => s.switchTab);

  const activeTab = tabs.find((t) => t.id === activeTabId) ?? null;

  // Local navigation state for the pre-tab flow
  const [navState, setNavState] = useState<FilesNavState>({ step: "accounts" });
  const [isCreatingTab, setIsCreatingTab] = useState(false);
  const navAccountHomeId =
    navState.step === "hub" ||
    navState.step === "sharepoint-sites" ||
    navState.step === "drive-list"
      ? navState.account.homeAccountId
      : "";

  // When an account is double-clicked, go to DriveHubPage
  const handleAccountSelect = (_driveId: string, _driveName: string, _cloudEnv: CloudEnvironment, account?: AccountEntry) => {
    if (account) {
      setIsCreatingTab(true);
      setNavState({ step: "hub", account });
    }
  };

  // When a drive is selected (from hub, drive list, etc.), open it as a tab
  const handleDriveSelect = (
    driveId: string,
    driveName: string,
    cloudEnv: CloudEnvironment,
    homeAccountId?: string
  ) => {
    openTab(driveId, driveName, cloudEnv, homeAccountId ?? navAccountHomeId);
    setIsCreatingTab(false);
  };

  const handleOpenPreview = (item: DriveItem) => {
    if (activeTab) {
      openPreviewTab(item, activeTab.driveId, activeTab.cloudEnv, activeTab.homeAccountId);
    }
  };

  const handleNewTab = () => {
    setNavState({ step: "accounts" });
    setIsCreatingTab(true);
  };

  const handleSwitchTab = (tabId: string) => {
    setIsCreatingTab(false);
    switchTab(tabId);
  };

  const handleCloseTab = (tabId: string) => {
    closeTab(tabId);
    if (useTabStore.getState().tabs.length === 0) {
      setIsCreatingTab(false);
      setNavState({ step: "accounts" });
    }
  };

  const showActiveTab = activeTab && !isCreatingTab;

  return (
    <div className="flex flex-col h-full">
      {tabs.length > 0 && (
        <TabBar
          onNewTab={handleNewTab}
          onTabSelect={handleSwitchTab}
          onCloseTab={handleCloseTab}
        />
      )}

      {showActiveTab && activeTab ? (
        <div className="flex-1 min-h-0 pt-2">
          {activeTab.kind === "preview" ? (
            <PreviewPage tab={activeTab} />
          ) : (
            <FileBrowser
              key={activeTab.id}
              tabId={activeTab.id}
              driveId={activeTab.driveId}
              homeAccountId={activeTab.homeAccountId}
              cloudEnv={activeTab.cloudEnv}
              driveName={activeTab.driveName}
              onOpenPreview={handleOpenPreview}
            />
          )}
        </div>
      ) : (
        <div className="flex-1 min-h-0 overflow-auto pt-2">
          {navState.step === "hub" && (
            <DriveHubPage
              account={navState.account}
              onDriveSelect={handleDriveSelect}
              onSharePointSelect={() =>
                setNavState({ step: "sharepoint-sites", account: navState.account })
              }
              onSharedSelect={() =>
                setNavState({ step: "drive-list", account: navState.account, mode: "shared" })
              }
              onBack={() => setNavState({ step: "accounts" })}
            />
          )}

          {navState.step === "sharepoint-sites" && (
            <SharePointSites
              cloudEnv={navState.account.cloudType}
              homeAccountId={navState.account.homeAccountId}
              onSiteSelect={(site) =>
                setNavState({
                  step: "drive-list",
                  account: navState.account,
                  mode: "sharepoint",
                  site,
                })
              }
              onBack={() => setNavState({ step: "hub", account: navState.account })}
            />
          )}

          {navState.step === "drive-list" && (
            <DriveList
              mode={navState.mode}
              siteId={navState.site?.id}
              siteName={navState.site?.displayName}
              cloudEnv={navState.account.cloudType}
              homeAccountId={navState.account.homeAccountId}
              onDriveSelect={handleDriveSelect}
              onBack={() => {
                if (navState.mode === "sharepoint") {
                  setNavState({ step: "sharepoint-sites", account: navState.account });
                } else {
                  setNavState({ step: "hub", account: navState.account });
                }
              }}
            />
          )}

          {navState.step === "accounts" && (
            <AccountList onDriveSelect={handleAccountSelect} />
          )}
        </div>
      )}
    </div>
  );
}

function TaskManagerPage() {
  return <TaskManager />;
}

function ToolsPage() {
  return <ToolsPageComponent />;
}

function SettingsPage() {
  const { t } = useTranslation();
  const theme = useSettingsStore((s) => s.theme);
  const setTheme = useSettingsStore((s) => s.setTheme);
  const language = useSettingsStore((s) => s.language);
  const setLanguage = useSettingsStore((s) => s.setLanguage);
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    getVersion().then(setVersion).catch(() => setVersion(null));
  }, []);

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-foreground">{t("settings.title")}</h2>
        <p className="text-muted-foreground">{t("settings.description")}</p>
      </div>

      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h3 className="text-lg font-semibold text-foreground">{t("settings.appearance")}</h3>
        <label className="text-sm text-muted-foreground" htmlFor="theme-setting">
          {t("settings.theme")}
        </label>
        <div id="theme-setting" className="flex w-fit gap-1 rounded-md border border-border bg-background p-0.5">
          {(["light", "dark", "system"] as const).map((mode) => (
            <button
              key={mode}
              onClick={() => setTheme(mode)}
              className={`rounded px-3 py-1.5 text-sm font-medium transition-colors ${
                theme === mode
                  ? "bg-primary text-primary-foreground"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground"
              }`}
            >
              {mode === "light"
                ? t("settings.themeLight")
                : mode === "dark"
                  ? t("settings.themeDark")
                  : t("settings.themeSystem")}
            </button>
          ))}
        </div>
      </section>

      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h3 className="text-lg font-semibold text-foreground">{t("settings.language")}</h3>
        <label className="text-sm text-muted-foreground" htmlFor="language-setting">
          {t("settings.language")}
        </label>
        <select
          id="language-setting"
          value={language}
          onChange={(e) => setLanguage(e.target.value)}
          className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground focus:outline-none focus:ring-2 focus:ring-ring"
        >
          <option value="system">{t("settings.languageSystem")}</option>
          <option value="en-US">English</option>
          <option value="zh-CN">简体中文</option>
        </select>
      </section>

      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h3 className="text-lg font-semibold text-foreground">{t("settings.about")}</h3>
        <p className="text-sm text-muted-foreground">{t("settings.aboutDescription")}</p>
        <div className="flex items-center justify-between rounded-md bg-muted/40 px-3 py-2">
          <span className="text-sm text-muted-foreground">{t("settings.version")}</span>
          <span className="text-sm font-medium text-foreground">{version ?? "..."}</span>
        </div>
      </section>

      <UpdateChecker />
    </div>
  );
}

export function MainContent() {
  const activeSection = useNavigationStore((s) => s.activeSection);

  switch (activeSection) {
    case "home":
      return <HomePage />;
    case "files":
      return <FilesPage />;
    case "bookmarks":
      return <BookmarksPage />;
    case "tasks":
      return <TaskManagerPage />;
    case "tools":
      return <ToolsPage />;
    case "settings":
      return <SettingsPage />;
    default:
      return <HomePage />;
  }
}
