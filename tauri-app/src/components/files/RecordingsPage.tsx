import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertCircle,
  ArrowLeft,
  Download,
  ExternalLink,
  LayoutGrid,
  List as ListIcon,
  Loader2,
  MonitorPlay,
  Play,
  RefreshCw,
  Search,
} from "lucide-react";
import { dirname, downloadDir, join } from "@tauri-apps/api/path";
import { save } from "@tauri-apps/plugin-dialog";
import { downloadFile, getMeetingRecordings, getThumbnailUrl } from "../../lib/tauri";
import type { AccountEntry, MeetingRecording } from "../../lib/types";
import { formatFileSize, formatDate } from "../../lib/formatters";
import { useSettingsStore } from "../../stores/settingsStore";
import { useTaskStore } from "../../stores/taskStore";
import { useAuthStore } from "../../stores/authStore";
import { useToastStore } from "../../stores/toastStore";
import { getErrorMessage, isAuthError } from "../../lib/errors";

/** View modes of the recordings page. */
export type RecordingsViewMode = "list" | "thumbnails";

interface RecordingsPageProps {
  account: AccountEntry;
  /** Open the recording in a new player tab. */
  onOpenRecording: (recording: MeetingRecording) => void;
  onBack: () => void;
}

/** Toolbar toggle shared style helper. */
function viewButtonClass(active: boolean): string {
  return `rounded-md p-1.5 transition-colors ${
    active ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:bg-accent"
  }`;
}

const thumbnailCache = new Map<string, string | null>();

/** Thumbnail with graceful fallback to a video glyph. */
function RecordingThumbnail({
  recording,
  cloudEnv,
  iconClassName,
  imgClassName,
}: {
  recording: MeetingRecording;
  cloudEnv: AccountEntry["cloudType"];
  iconClassName: string;
  imgClassName: string;
}) {
  const { driveId, item } = recording;
  const [thumbnailUrl, setThumbnailUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const key = `${cloudEnv}:${driveId}:${item.id}`;
    const cached = thumbnailCache.get(key);
    if (cached !== undefined) {
      setThumbnailUrl(cached);
      setFailed(cached === null);
      return;
    }

    let cancelled = false;
    getThumbnailUrl(driveId, item.id, cloudEnv)
      .then((url) => {
        if (cancelled) return;
        thumbnailCache.set(key, url);
        setThumbnailUrl(url);
        setFailed(false);
      })
      .catch(() => {
        if (cancelled) return;
        thumbnailCache.set(key, null);
        setThumbnailUrl(null);
        setFailed(true);
      });

    return () => {
      cancelled = true;
    };
  }, [item.id, driveId, cloudEnv]);

  if (!thumbnailUrl || failed) {
    return <MonitorPlay className={`shrink-0 text-purple-500 ${iconClassName}`} />;
  }

  return (
    <img
      src={thumbnailUrl}
      alt=""
      loading="lazy"
      className={imgClassName}
      onError={() => {
        thumbnailCache.set(`${cloudEnv}:${driveId}:${item.id}`, null);
        setThumbnailUrl(null);
        setFailed(true);
      }}
    />
  );
}

function sourceLabel(recording: MeetingRecording, t: (key: string) => string): string {
  if (recording.sourceType === "onedrive") {
    return t("recordings.sourceOneDrive");
  }
  if (recording.sourceType === "search") {
    return t("recordings.sourceSearch");
  }
  return recording.sourceName || t("recordings.sourceSharePoint");
}

/** Page listing all Teams meeting recordings the account can access. */
export function RecordingsPage({ account, onOpenRecording, onBack }: RecordingsPageProps) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const lastDownloadPath = useSettingsStore((s) => s.lastDownloadPath);
  const setLastDownloadPath = useSettingsStore((s) => s.setLastDownloadPath);
  const pendingRelogin = useAuthStore((s) => s.pendingRelogin);

  const [recordings, setRecordings] = useState<MeetingRecording[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  /** Set when loading hit an auth error and the global relogin dialog was raised. */
  const [awaitingRelogin, setAwaitingRelogin] = useState(false);
  const [viewMode, setViewMode] = useState<RecordingsViewMode>("list");
  const [filterText, setFilterText] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [downloadingId, setDownloadingId] = useState<string | null>(null);
  const prevPendingReloginRef = useRef(pendingRelogin);

  const load = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    setAwaitingRelogin(false);
    setSelectedId(null);
    try {
      const result = await getMeetingRecordings(account.cloudType, account.homeAccountId);
      setRecordings(result);
    } catch (err) {
      if (isAuthError(err)) {
        // Raise the app-wide relogin dialog and wait for its completion.
        setAwaitingRelogin(true);
        useAuthStore.getState().setPendingRelogin({ cloudEnv: account.cloudType });
        return;
      }
      setError(getErrorMessage(err));
    } finally {
      setIsLoading(false);
    }
  }, [account.cloudType, account.homeAccountId]);

  useEffect(() => {
    load();
  }, [load]);

  // Reload automatically once the relogin dialog resolves successfully.
  useEffect(() => {
    const prev = prevPendingReloginRef.current;
    prevPendingReloginRef.current = pendingRelogin;
    if (awaitingRelogin && prev !== null && pendingRelogin === null) {
      load();
    }
  }, [pendingRelogin, awaitingRelogin, load]);

  const visible = filterText.trim()
    ? recordings.filter((recording) => {
        const needle = filterText.trim().toLowerCase();
        return (
          recording.item.name.toLowerCase().includes(needle) ||
          sourceLabel(recording, t).toLowerCase().includes(needle)
        );
      })
    : recordings;

  const handleDownload = useCallback(
    async (recording: MeetingRecording) => {
      if (downloadingId) return;
      setDownloadingId(recording.item.id);
      try {
        const dir = lastDownloadPath ?? (await downloadDir());
        const defaultPath = await join(dir, recording.item.name);
        const selected = await save({ defaultPath });
        if (!selected) return;
        setLastDownloadPath(await dirname(selected));
        const batch = await downloadFile(
          recording.driveId,
          recording.item.id,
          account.homeAccountId,
          recording.item.name,
          recording.item.size ?? 0,
          selected,
          account.cloudType
        );
        useTaskStore.getState().registerTask(batch.batchId, {
          type: "download",
          fileName: batch.batchName,
          homeAccountId: account.homeAccountId,
          driveId: recording.driveId,
          cloudEnv: account.cloudType,
          itemId: recording.item.id,
          localPath: selected,
        });
        addToast("success", t("recordings.downloadStarted"));
      } catch (err) {
        addToast("error", getErrorMessage(err));
      } finally {
        setDownloadingId(null);
      }
    },
    [account.cloudType, account.homeAccountId, downloadingId, lastDownloadPath, setLastDownloadPath, addToast, t]
  );

  const handleOpen = useCallback(
    (recording: MeetingRecording) => {
      setSelectedId(recording.item.id);
      onOpenRecording(recording);
    },
    [onOpenRecording]
  );

  const rowActions = (recording: MeetingRecording) => (
    <div className="flex shrink-0 items-center gap-0.5">
      <button
        onClick={(e) => {
          e.stopPropagation();
          handleOpen(recording);
        }}
        className="rounded-md p-1.5 text-muted-foreground opacity-0 transition-all hover:bg-accent hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100 group-hover:pointer-events-auto"
        title={t("recordings.play")}
        aria-label={t("recordings.play")}
      >
        <Play className="h-3.5 w-3.5" />
      </button>
      <button
        onClick={(e) => {
          e.stopPropagation();
          handleDownload(recording);
        }}
        disabled={downloadingId === recording.item.id}
        className="rounded-md p-1.5 text-muted-foreground opacity-0 transition-all hover:bg-accent hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100 group-hover:pointer-events-auto disabled:cursor-not-allowed disabled:opacity-40"
        title={t("fileOps.download")}
        aria-label={t("fileOps.download")}
      >
        <Download className="h-3.5 w-3.5" />
      </button>
      {recording.item.webUrl && (
        <a
          href={recording.item.webUrl}
          target="_blank"
          rel="noreferrer"
          onClick={(e) => e.stopPropagation()}
          className="rounded-md p-1.5 text-muted-foreground opacity-0 transition-all hover:bg-accent hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100 group-hover:pointer-events-auto"
          title={t("preview.openInBrowser")}
          aria-label={t("preview.openInBrowser")}
        >
          <ExternalLink className="h-3.5 w-3.5" />
        </a>
      )}
    </div>
  );

  const isFiltered = filterText.trim().length > 0;

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <button
            onClick={onBack}
            className="rounded-md p-1.5 text-muted-foreground hover:bg-accent transition-colors"
            aria-label={t("files.back")}
            title={t("files.back")}
          >
            <ArrowLeft className="h-5 w-5" />
          </button>
          <h3 className="text-lg font-semibold text-foreground">{t("recordings.title")}</h3>
          {!isLoading && !error && (
            <span className="text-sm text-muted-foreground">
              {visible.length}/{recordings.length}
            </span>
          )}
        </div>

        <div className="flex items-center gap-2">
          {/* Local filter */}
          <div className="relative">
            <Search className="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
            <input
              value={filterText}
              onChange={(e) => setFilterText(e.target.value)}
              placeholder={t("recordings.filterPlaceholder")}
              className="w-48 rounded-md border border-border bg-background py-1.5 pl-8 pr-3 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring"
              aria-label={t("recordings.filterPlaceholder")}
            />
          </div>

          <button
            onClick={load}
            disabled={isLoading}
            className="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors disabled:opacity-50"
            aria-label={t("files.refresh")}
            title={t("files.refresh")}
          >
            <RefreshCw className={`h-4 w-4 ${isLoading ? "animate-spin" : ""}`} />
          </button>

          {/* View mode switcher */}
          <div className="flex items-center gap-1 rounded-md border border-border p-0.5">
            <button
              onClick={() => setViewMode("list")}
              className={viewButtonClass(viewMode === "list")}
              aria-label={t("recordings.layoutList")}
              title={t("recordings.layoutList")}
            >
              <ListIcon className="h-4 w-4" />
            </button>
            <button
              onClick={() => setViewMode("thumbnails")}
              className={viewButtonClass(viewMode === "thumbnails")}
              aria-label={t("recordings.layoutThumbnails")}
              title={t("recordings.layoutThumbnails")}
            >
              <LayoutGrid className="h-4 w-4" />
            </button>
          </div>
        </div>
      </div>

      {/* Content */}
      <div className="min-h-0 flex-1 overflow-auto">
        {isLoading && (
          <div className="flex flex-col items-center gap-3 py-16">
            <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
            <p className="text-sm text-muted-foreground">{t("recordings.loading")}</p>
          </div>
        )}

        {!isLoading && error && (
          <div className="flex flex-col items-center justify-center gap-3 py-16">
            <AlertCircle className="h-8 w-8 text-destructive" />
            <p className="max-w-xl text-center text-sm text-muted-foreground">{error}</p>
            <button
              onClick={load}
              className="rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
            >
              {t("errors.retryAction")}
            </button>
          </div>
        )}

        {!isLoading && !error && awaitingRelogin && (
          <div className="flex flex-col items-center justify-center gap-3 py-16">
            <MonitorPlay className="h-10 w-10 text-muted-foreground/50" />
            <p className="text-sm text-muted-foreground">{t("recordings.reloginRequired")}</p>
          </div>
        )}

        {!isLoading && !error && !awaitingRelogin && visible.length === 0 && (
          <div className="flex flex-col items-center justify-center gap-3 py-16">
            <MonitorPlay className="h-10 w-10 text-muted-foreground/50" />
            <p className="text-sm text-muted-foreground">
              {isFiltered ? t("recordings.noMatches") : t("recordings.empty")}
            </p>
          </div>
        )}

        {!isLoading && !error && !awaitingRelogin && visible.length > 0 && (
          <>
            {viewMode === "list" ? (
              <>
                <div className="flex items-center gap-3 border-b border-border px-3 py-1 text-xs font-medium text-muted-foreground">
                  <span className="flex-1">{t("fileBrowser.name")}</span>
                  <span className="w-40 truncate">{t("recordings.source")}</span>
                  <span className="w-24 text-right">{t("fileBrowser.size")}</span>
                  <span className="w-36 text-right">{t("recordings.created")}</span>
                  <span className="w-[70px]" />
                </div>
                <div className="flex flex-col">
                  {visible.map((recording) => (
                    <div
                      key={`${recording.driveId}:${recording.item.id}`}
                      role="row"
                      aria-label={recording.item.name}
                      tabIndex={0}
                      onClick={() => setSelectedId(recording.item.id)}
                      onDoubleClick={() => handleOpen(recording)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") handleOpen(recording);
                      }}
                      className={`group flex cursor-pointer select-none items-center gap-3 rounded-md px-3 py-2 transition-colors hover:bg-accent/50 ${
                        selectedId === recording.item.id ? "bg-accent/40" : ""
                      }`}
                    >
                      <RecordingThumbnail
                        recording={recording}
                        cloudEnv={account.cloudType}
                        iconClassName="h-5 w-5"
                        imgClassName="h-5 w-5 shrink-0 rounded object-cover"
                      />
                      <span className="flex-1 truncate text-sm text-foreground" title={recording.item.name}>
                        {recording.item.name}
                      </span>
                      <span className="w-40 truncate text-xs text-muted-foreground" title={sourceLabel(recording, t)}>
                        {sourceLabel(recording, t)}
                      </span>
                      <span className="w-24 text-right text-xs text-muted-foreground">
                        {formatFileSize(recording.item.size ?? 0)}
                      </span>
                      <span className="w-36 text-right text-xs text-muted-foreground">
                        {formatDate(recording.item.createdDateTime ?? recording.item.lastModified)}
                      </span>
                      {rowActions(recording)}
                    </div>
                  ))}
                </div>
              </>
            ) : (
              <div className="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-3">
                {visible.map((recording) => (
                  <div
                    key={`${recording.driveId}:${recording.item.id}`}
                    role="gridcell"
                    aria-label={recording.item.name}
                    tabIndex={0}
                    onClick={() => setSelectedId(recording.item.id)}
                    onDoubleClick={() => handleOpen(recording)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") handleOpen(recording);
                    }}
                    className={`group flex cursor-pointer select-none flex-col overflow-hidden rounded-lg border border-border bg-card transition-colors hover:border-primary/50 hover:bg-accent/30 ${
                      selectedId === recording.item.id ? "ring-2 ring-primary" : ""
                    }`}
                  >
                    <div className="flex h-28 items-center justify-center bg-muted/30">
                      <RecordingThumbnail
                        recording={recording}
                        cloudEnv={account.cloudType}
                        iconClassName="h-12 w-12"
                        imgClassName="h-full w-full object-cover"
                      />
                    </div>
                    <div className="relative p-2 pb-7">
                      <span className="block truncate text-xs text-foreground" title={recording.item.name}>
                        {recording.item.name}
                      </span>
                      <span className="absolute bottom-2 left-2 block max-w-[calc(100%-1rem)] truncate text-xs text-muted-foreground">
                        {sourceLabel(recording, t)} · {formatFileSize(recording.item.size ?? 0)}
                      </span>
                      <div className="absolute bottom-1 right-1 bg-card/90">{rowActions(recording)}</div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
