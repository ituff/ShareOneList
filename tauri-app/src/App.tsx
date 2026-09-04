import { Suspense, useEffect } from "react";
import { Sidebar } from "./components/layout/Sidebar";
import { MainContent } from "./components/layout/MainContent";
import { ToastContainer } from "./components/ui/Toast";
import { ReloginDialog } from "./components/accounts/ReloginDialog";
import { NotificationBell } from "./components/layout/NotificationBell";
import { UpdateBubble } from "./components/layout/UpdateBubble";
import { useWindowState } from "./hooks/useWindowState";
import { useTheme } from "./hooks/useTheme";
import { useSettingsStore } from "./stores/settingsStore";
import { useAuthStore } from "./stores/authStore";
import { initTaskListener, useTaskStore } from "./stores/taskStore";
import { syncLanguageFromStore } from "./i18n";

function App() {
  // Restore window position/size from saved config and persist changes on resize/move
  useWindowState();

  // Apply and sync theme (dark/light/system) based on settings store
  useTheme();

  const language = useSettingsStore((s) => s.language);
  const isLoaded = useSettingsStore((s) => s.isLoaded);
  const loadConfig = useSettingsStore((s) => s.loadConfig);
  const loadAccounts = useAuthStore((s) => s.loadAccounts);
  const loadDownloadTasks = useTaskStore((s) => s.loadDownloadTasks);

  // Load persisted application config (theme, language, window, download path)
  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  // Load cached accounts from backend on startup
  useEffect(() => {
    loadAccounts();
  }, [loadAccounts]);

  // Initialize Tauri event listener for transfer progress
  useEffect(() => {
    initTaskListener();
  }, []);

  // Restore paused/interrupted download batches after app restart
  useEffect(() => {
    loadDownloadTasks();
  }, [loadDownloadTasks]);

  // Sync i18n language with settings store once config is loaded
  useEffect(() => {
    if (isLoaded) {
      syncLanguageFromStore(language);
    }
  }, [isLoaded, language]);

  return (
    <Suspense fallback={<div className="flex h-screen w-screen items-center justify-center bg-background text-foreground">Loading...</div>}>
      <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
        <Sidebar />
        <main className="flex-1 overflow-auto p-6">
          <MainContent />
        </main>
      </div>
      <ToastContainer />
      <ReloginDialog />
      <NotificationBell />
      <UpdateBubble />
    </Suspense>
  );
}

export default App;
