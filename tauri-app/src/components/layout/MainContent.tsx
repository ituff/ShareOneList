import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigationStore } from "../../stores/navigationStore";
import { AccountList } from "../accounts/AccountList";
import { FileBrowser } from "../files/FileBrowser";
import { DriveHubPage } from "../files/DriveHubPage";
import { SharePointSites } from "../files/SharePointSites";
import { DriveList } from "../files/DriveList";
import { TabBar } from "./TabBar";
import { useTabStore } from "../../stores/tabStore";
import { TaskManager } from "../tasks/TaskManager";
import { ToolsPage as ToolsPageComponent } from "../tools/ToolsPage";
import { UpdateChecker } from "../tools/UpdateChecker";
import type { AccountEntry, CloudEnvironment, Site } from "../../lib/types";

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
  const closeTab = useTabStore((s) => s.closeTab);

  const activeTab = tabs.find((t) => t.id === activeTabId) ?? null;

  // Local navigation state for the pre-tab flow
  const [navState, setNavState] = useState<FilesNavState>({ step: "accounts" });

  // When an account is double-clicked, go to DriveHubPage
  const handleAccountSelect = (_driveId: string, _driveName: string, _cloudEnv: CloudEnvironment, account?: AccountEntry) => {
    if (account) {
      setNavState({ step: "hub", account });
    }
  };

  // When a drive is selected (from hub, drive list, etc.), open it as a tab
  const handleDriveSelect = (driveId: string, driveName: string, cloudEnv: CloudEnvironment) => {
    openTab(driveId, driveName, cloudEnv);
  };

  // When clicking back from FileBrowser, close the active tab
  const handleBack = () => {
    if (activeTabId) {
      closeTab(activeTabId);
    }
  };

  // If there's an active tab, show TabBar + FileBrowser
  if (activeTab) {
    return (
      <div className="flex flex-col h-full">
        <TabBar />
        <div className="flex-1 min-h-0 pt-2">
          <FileBrowser
            key={activeTab.id}
            driveId={activeTab.driveId}
            cloudEnv={activeTab.cloudEnv}
            driveName={activeTab.driveName}
            onBack={handleBack}
          />
        </div>
      </div>
    );
  }

  // Pre-tab navigation flow
  switch (navState.step) {
    case "hub":
      return (
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
      );

    case "sharepoint-sites":
      return (
        <SharePointSites
          cloudEnv={navState.account.cloudType}
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
      );

    case "drive-list":
      return (
        <DriveList
          mode={navState.mode}
          siteId={navState.site?.id}
          siteName={navState.site?.displayName}
          cloudEnv={navState.account.cloudType}
          onDriveSelect={handleDriveSelect}
          onBack={() => {
            if (navState.mode === "sharepoint") {
              setNavState({ step: "sharepoint-sites", account: navState.account });
            } else {
              setNavState({ step: "hub", account: navState.account });
            }
          }}
        />
      );

    case "accounts":
    default:
      return <AccountList onDriveSelect={handleAccountSelect} />;
  }
}

function TaskManagerPage() {
  return <TaskManager />;
}

function ToolsPage() {
  return <ToolsPageComponent />;
}

function SettingsPage() {
  const { t } = useTranslation();
  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-foreground">{t("settings.title")}</h2>
        <p className="text-muted-foreground">{t("settings.description")}</p>
      </div>
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
