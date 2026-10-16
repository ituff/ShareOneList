import { useTranslation } from "react-i18next";
import { Download } from "lucide-react";
import { useUpdateStore } from "../../stores/updateStore";
import { formatFileSize } from "../../lib/formatters";

/**
 * Compact floating pill shown while an update downloads in the background and
 * the bubble is dismissed, so progress stays visible on every screen. Clicking
 * it reopens the update bubble.
 */
export function UpdateDownloadIndicator() {
  const { t } = useTranslation();
  const downloading = useUpdateStore((s) => s.downloading);
  const dismissed = useUpdateStore((s) => s.dismissed);
  const transferred = useUpdateStore((s) => s.transferred);
  const total = useUpdateStore((s) => s.total);
  const speed = useUpdateStore((s) => s.speed);
  const reopen = useUpdateStore((s) => s.reopen);

  if (!downloading || !dismissed) return null;

  const percent = total > 0 ? Math.min(100, Math.round((transferred / total) * 100)) : 0;

  return (
    <button
      onClick={reopen}
      title={t("update.backgroundTitle")}
      className="fixed bottom-4 right-4 z-[85] flex items-center gap-2 rounded-full border border-border bg-card px-3 py-2 shadow-lg transition-colors hover:bg-accent"
    >
      <Download className="h-3.5 w-3.5 shrink-0 text-primary" />
      <span className="text-xs font-medium text-foreground">
        {total > 0 ? `${percent}%` : formatFileSize(transferred)}
      </span>
      {speed > 0 && (
        <span className="text-xs text-muted-foreground">
          {t("update.speed", { value: formatFileSize(speed) })}
        </span>
      )}
      <span className="absolute bottom-0 left-3 right-3 h-0.5 overflow-hidden rounded-full bg-muted">
        <span
          className="block h-full rounded-full bg-primary transition-all"
          style={{ width: `${percent}%` }}
        />
      </span>
    </button>
  );
}
