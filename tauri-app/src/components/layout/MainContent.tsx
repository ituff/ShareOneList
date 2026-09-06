import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import wechatQrCode from "../../assets/wechat-qrcode.png";
import { useNavigationStore } from "../../stores/navigationStore";
import { AccountList } from "../accounts/AccountList";
import { BookmarksPage } from "../bookmarks/BookmarksPage";
import { FileBrowser } from "../files/FileBrowser";
import { PreviewPage } from "../files/PreviewPage";
import { DriveHubPage } from "../files/DriveHubPage";
import { RecordingsPage } from "../files/RecordingsPage";
import { SharePointSites } from "../files/SharePointSites";
import { DriveList } from "../files/DriveList";
import { TabBar } from "./TabBar";
import { useTabStore } from "../../stores/tabStore";
import { useSettingsStore } from "../../stores/settingsStore";
import { useAuthStore } from "../../stores/authStore";
import { TaskManager } from "../tasks/TaskManager";
import { ToolsPage as ToolsPageComponent } from "../tools/ToolsPage";
import { UpdateChecker } from "../tools/UpdateChecker";
import { HomePage } from "../home/HomePage";
import { LlmSettings } from "../settings/LlmSettings";
import type {
  AccountEntry,
  CloudEnvironment,
  DriveItem,
  MeetingRecording,
  Site,
  TabState,
} from "../../lib/types";

/** Navigation steps within the files page (before a tab is opened). */
type FilesNavState =
  | { step: "accounts" }
  | { step: "hub"; account: AccountEntry }
  | { step: "sharepoint-sites"; account: AccountEntry }
  | { step: "drive-list"; account: AccountEntry; site: Site };

function FilesPage() {
  const { t } = useTranslation();
  const tabs = useTabStore((s) => s.tabs);
  const activeTabId = useTabStore((s) => s.activeTabId);
  const openTab = useTabStore((s) => s.openTab);
  const openPreviewTab = useTabStore((s) => s.openPreviewTab);
  const openRecordingsTab = useTabStore((s) => s.openRecordingsTab);
  const closeTab = useTabStore((s) => s.closeTab);
  const switchTab = useTabStore((s) => s.switchTab);
  const accounts = useAuthStore((s) => s.accounts);

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
  const handleAccountSelect = (
    _driveId: string,
    _driveName: string,
    _cloudEnv: CloudEnvironment,
    account?: AccountEntry
  ) => {
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

  /** Open a meeting recording from the recordings tab in a player tab. */
  const handleOpenRecording = (recording: MeetingRecording) => {
    if (!activeTab || activeTab.kind !== "recordings") return;
    openPreviewTab(
      recording.item,
      recording.driveId,
      activeTab.cloudEnv,
      activeTab.homeAccountId
    );
  };

  const handleNewTab = () => {
    setNavState({ step: "accounts" });
    setIsCreatingTab(true);
  };

  /** Back from a tab's root: show the account's service selection page (the
   * hub) as an overlay; the tab itself stays open in the tab bar. */
  const handleExitToHub = (tab: TabState) => {
    const account = accounts.find(
      (a) => a.homeAccountId === tab.homeAccountId && a.cloudType === tab.cloudEnv
    );
    const fallback: AccountEntry = {
      homeAccountId: tab.homeAccountId,
      driveId: tab.driveId,
      cloudType: tab.cloudEnv,
      displayName: tab.driveName,
      accountType: null,
    };
    setNavState({ step: "hub", account: account ?? fallback });
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

  /** The pre-tab navigation flow, also overlaid while creating a new tab. */
  const navPages = (
    <>
      {navState.step === "hub" && (
        <DriveHubPage
          account={navState.account}
          onDriveSelect={handleDriveSelect}
          onSharePointSelect={() =>
            setNavState({ step: "sharepoint-sites", account: navState.account })
          }
          onMeetingsSelect={() => {
            openRecordingsTab(
              navState.account.homeAccountId,
              navState.account.cloudType,
              t("recordings.title")
            );
            setIsCreatingTab(false);
          }}
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
              site,
            })
          }
          onBack={() => setNavState({ step: "hub", account: navState.account })}
        />
      )}

{navState.step === "drive-list" && (
	        <DriveList
	          siteId={navState.site.id}
	          siteName={navState.site.displayName}
	          cloudEnv={navState.account.cloudType}
	          homeAccountId={navState.account.homeAccountId}
	          onDriveSelect={handleDriveSelect}
	          onBack={() =>
	            setNavState({ step: "sharepoint-sites", account: navState.account })
	          }
	        />
	      )}

      {navState.step === "accounts" && <AccountList onDriveSelect={handleAccountSelect} />}
    </>
  );

  return (
    <div className="flex flex-col h-full">
      {tabs.length > 0 && (
        <TabBar
          onNewTab={handleNewTab}
          onTabSelect={handleSwitchTab}
          onCloseTab={handleCloseTab}
        />
      )}

      {/* All tabs stay mounted (keep-alive) so background videos keep playing and
          players survive tab switches; inactive tabs are hidden via display:none. */}
      {tabs.length > 0 && (
        <div className="relative min-h-0 flex-1 pt-2">
          {tabs.map((tab) => {
            const isActive = tab.id === activeTabId && !isCreatingTab;
            return (
              <div
                key={tab.id}
                className={`absolute inset-0 flex min-h-0 flex-col ${isActive ? "" : "hidden"}`}
              >
                {tab.kind === "preview" ? (
                  <PreviewPage tab={tab} />
                ) : tab.kind === "recordings" ? (
                  <RecordingsPage
                    account={{
                      homeAccountId: tab.homeAccountId,
                      driveId: "",
                      cloudType: tab.cloudEnv,
                      displayName: tab.driveName,
                    }}
                    onOpenRecording={handleOpenRecording}
                    onBack={() => handleExitToHub(tab)}
                  />
                ) : (
                  <FileBrowser
                    tabId={tab.id}
                    isActive={isActive}
                    driveId={tab.driveId}
                    homeAccountId={tab.homeAccountId}
                    cloudEnv={tab.cloudEnv}
                    driveName={tab.driveName}
                    onOpenPreview={handleOpenPreview}
                    onExitToHub={() => handleExitToHub(tab)}
                  />
                )}
              </div>
            );
          })}
          {isCreatingTab && (
            <div className="absolute inset-0 z-40 overflow-auto bg-background pt-2">
              {navPages}
            </div>
          )}
        </div>
      )}

      {tabs.length === 0 && (
        <div className="min-h-0 flex-1 overflow-auto pt-2">{navPages}</div>
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
  const segmentConcurrency = useSettingsStore((s) => s.segmentDownloadConcurrency);
  const setSegmentConcurrency = useSettingsStore((s) => s.setSegmentDownloadConcurrency);
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

      <LlmSettings />

      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h3 className="text-lg font-semibold text-foreground">{t("settings.downloads")}</h3>
        <label className="text-sm text-muted-foreground" htmlFor="segment-concurrency-setting">
          {t("settings.segmentConcurrency")}
        </label>
        <select
          id="segment-concurrency-setting"
          value={segmentConcurrency}
          onChange={(e) => setSegmentConcurrency(Number(e.target.value))}
          className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground focus:outline-none focus:ring-2 focus:ring-ring"
        >
          {[1, 4, 8, 16].map((n) => (
            <option key={n} value={n}>
              {t("settings.segmentConcurrencyOption", { count: n })}
            </option>
          ))}
        </select>
        <p className="text-xs text-muted-foreground">{t("settings.segmentConcurrencyHint")}</p>
      </section>

      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h3 className="text-lg font-semibold text-foreground">{t("settings.about")}</h3>
        <p className="text-sm text-muted-foreground">{t("settings.aboutDescription")}</p>
        <div className="flex items-center justify-between rounded-md bg-muted/40 px-3 py-2">
          <span className="text-sm text-muted-foreground">{t("settings.github")}</span>
          <a
            href="https://github.com/ituff/ShareOneList"
            target="_blank"
            rel="noreferrer"
            className="text-sm font-medium text-primary hover:underline"
          >
            github.com/ituff/ShareOneList
          </a>
        </div>
        <div className="flex items-center justify-between rounded-md bg-muted/40 px-3 py-2">
          <span className="text-sm text-muted-foreground">{t("settings.version")}</span>
          <span className="text-sm font-medium text-foreground">{version ?? "..."}</span>
        </div>
      </section>

      {/* WeChat official account promotion */}
      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h3 className="text-lg font-semibold text-foreground">{t("settings.followTitle")}</h3>
        <p className="text-sm text-muted-foreground">{t("settings.followDescription")}</p>
        <img
          src={wechatQrCode}
          alt="WeChat: ONE生产力"
          className="w-40 rounded-md border border-border"
        />
      </section>

      <UpdateChecker />
    </div>
  );
}

export function MainContent() {
  const activeSection = useNavigationStore((s) => s.activeSection);

  switch (activeSection) {
    case "home":
      // key forces a remount per section: initialMode would otherwise be
      // ignored when React reuses the HomePage instance across sections.
      return <HomePage key="home" />;
    case "askai":
      return <HomePage key="askai" initialMode="chat" />;
    case "search":
      return <HomePage key="search" initialMode="search" />;
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
