import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { AlertCircle, Bell, CheckCircle, Trash2, Video } from "lucide-react";
import { useNotificationStore } from "../../stores/notificationStore";

type TFunc = (key: string, options?: Record<string, unknown>) => string;

/** Render a relative "x minutes ago" style timestamp. */
function formatAge(createdAt: number, t: TFunc): string {
  const minutes = Math.floor((Date.now() - createdAt) / 60000);
  if (minutes < 1) return t("notifications.justNow");
  if (minutes < 60) return t("notifications.minutesAgo", { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("notifications.hoursAgo", { count: hours });
  return t("notifications.daysAgo", { count: Math.floor(hours / 24) });
}

/**
 * Fixed notification bell at the top-right corner of the window.
 * Clicking toggles a dropdown panel listing recent notifications
 * (updates, download completions, download errors).
 */
export function NotificationBell() {
  const { t } = useTranslation();
  const { notifications, panelOpen, setPanelOpen, markAllRead, clearAll } =
    useNotificationStore();
  const containerRef = useRef<HTMLDivElement>(null);

  const unreadCount = notifications.filter((n) => !n.read).length;

  // Close the panel when clicking anywhere outside it.
  useEffect(() => {
    if (!panelOpen) return;
    const handler = (event: MouseEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) {
        setPanelOpen(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [panelOpen, setPanelOpen]);

  const kindIcon = (kind: string) => {
    if (kind === "download") {
      return <CheckCircle className="h-4 w-4 shrink-0 text-green-500" />;
    }
    if (kind === "download-error") {
      return <AlertCircle className="h-4 w-4 shrink-0 text-destructive" />;
    }
    return <Video className="h-4 w-4 shrink-0 text-primary" />;
  };

  return (
    <div ref={containerRef} className="fixed right-3 top-2 z-[90]">
      <button
        onClick={() => setPanelOpen(!panelOpen)}
        className="relative rounded-md p-2 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
        aria-label={t("notifications.title")}
        title={t("notifications.title")}
      >
        <Bell className="h-4 w-4" />
        {unreadCount > 0 && (
          <span className="absolute -right-0.5 -top-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[10px] font-medium leading-none text-destructive-foreground">
            {unreadCount > 9 ? "9+" : unreadCount}
          </span>
        )}
      </button>

      {panelOpen && (
        <div className="absolute right-0 top-10 w-80 overflow-hidden rounded-lg border border-border bg-card shadow-lg">
          <div className="flex items-center justify-between border-b border-border px-3 py-2">
            <span className="text-sm font-medium text-foreground">
              {t("notifications.title")}
            </span>
            <div className="flex items-center gap-1">
              <button
                onClick={markAllRead}
                className="rounded px-2 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
              >
                {t("notifications.markAllRead")}
              </button>
              <button
                onClick={clearAll}
                className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-destructive transition-colors"
                aria-label={t("notifications.clear")}
                title={t("notifications.clear")}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </button>
            </div>
          </div>

          <div className="max-h-80 overflow-auto">
            {notifications.length === 0 ? (
              <p className="px-3 py-6 text-center text-xs text-muted-foreground">
                {t("notifications.empty")}
              </p>
            ) : (
              notifications.map((n) => (
                <div
                  key={n.id}
                  className={`flex items-start gap-2 border-b border-border/60 px-3 py-2 last:border-b-0 ${
                    n.read ? "opacity-60" : ""
                  }`}
                >
                  <div className="mt-0.5">{kindIcon(n.kind)}</div>
                  <div className="min-w-0 flex-1">
                    <div className="text-xs font-medium text-foreground">
                      {n.title}
                    </div>
                    {n.detail && (
                      <div className="mt-0.5 break-all text-xs text-muted-foreground">
                        {n.detail}
                      </div>
                    )}
                    <div className="mt-0.5 text-[10px] text-muted-foreground/70">
                      {formatAge(n.createdAt, t)}
                    </div>
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  );
}
