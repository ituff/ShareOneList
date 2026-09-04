import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertCircle, Download, ExternalLink, Loader2, RotateCw, Video } from "lucide-react";
import { dirname, downloadDir, join } from "@tauri-apps/api/path";
import { save } from "@tauri-apps/plugin-dialog";
import {
  beginStreamDownload,
  downloadFile,
  probeDownloadAllowed,
  getPreviewUrl,
  getTextContent,
} from "../../lib/tauri";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { TabState } from "../../lib/types";
import { isImageFile, isMarkdownFile, isTextFile, isVideoFile } from "./FilePreview";
import { useSettingsStore } from "../../stores/settingsStore";
import { useTaskStore } from "../../stores/taskStore";
import { useToastStore } from "../../stores/toastStore";
import { getErrorMessage } from "../../lib/errors";
interface PreviewPageProps {
  tab: TabState;
}

type StreamState = "idle" | "downloading" | "done" | "error";

interface SolMessage {
  type: string;
  captureId?: string;
  done?: number;
  total?: number;
  text?: string;
  message?: string;
  isDrm?: boolean;
}

function isSolMessage(data: unknown): data is SolMessage {
  return (
    typeof data === "object" &&
    data !== null &&
    typeof (data as { type?: unknown }).type === "string" &&
    (data as { type: string }).type.startsWith("SOL_")
  );
}

/** Full-tab file preview backed by the Graph preview API. */
export function PreviewPage({ tab }: PreviewPageProps) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const lastDownloadPath = useSettingsStore((s) => s.lastDownloadPath);
  const setLastDownloadPath = useSettingsStore((s) => s.setLastDownloadPath);
  const segmentConcurrency = useSettingsStore((s) => s.segmentDownloadConcurrency);
  const item = tab.previewItem;
  const [previewUrl, setPreviewUrl] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isEmbed, setIsEmbed] = useState(false);
  const [isDownloading, setIsDownloading] = useState(false);
  const [textContent, setTextContent] = useState<string | null>(null);

  // Stream-capture download state (see stream_boot.js in the Rust crate).
  const iframeRef = useRef<HTMLIFrameElement | null>(null);
  const channelRef = useRef<{ port: number; uploadToken: string } | null>(null);
  const [capturedId, setCapturedId] = useState<string | null>(null);
  const [streamState, setStreamState] = useState<StreamState>("idle");
  const [streamProgress, setStreamProgress] = useState<string>("");
  const [streamErrorDetail, setStreamErrorDetail] = useState<string>("");
  /** True when tenant policy withholds the downloadUrl: downloads must go
   * through the stream-capture pipeline instead of the Graph download. */
  const [downloadBlocked, setDownloadBlocked] = useState(false);

  const loadPreview = useCallback(async () => {
    if (!item) return;
    setError(null);
    setIsEmbed(false);
    setPreviewUrl(null);
    setTextContent(null);

    if (isTextFile(item)) {
      setIsLoading(true);
      try {
        const content = await getTextContent(
          tab.driveId,
          item.id,
          tab.cloudEnv,
          tab.homeAccountId
        );
        setTextContent(content);
      } catch (err) {
        setError(getErrorMessage(err));
      } finally {
        setIsLoading(false);
      }
      return;
    }

    if (isImageFile(item) && item.downloadUrl) {
      setPreviewUrl(item.downloadUrl);
      return;
    }

    setIsLoading(true);
    try {
      const url = await getPreviewUrl(tab.driveId, item.id, tab.cloudEnv);
      setPreviewUrl(url);
      setIsEmbed(true);
    } catch {
      if (isVideoFile(item) && item.downloadUrl) {
        setPreviewUrl(item.downloadUrl);
      } else {
        const fallback = item.webUrl
          ? `${item.webUrl}${item.webUrl.includes("?") ? "&" : "?"}web=1`
          : null;
        if (fallback) {
          setPreviewUrl(fallback);
          setIsEmbed(true);
        } else {
          setError(t("preview.loadFailed"));
        }
      }
    } finally {
      setIsLoading(false);
    }
  }, [item, tab.driveId, tab.cloudEnv, tab.homeAccountId, t]);

  useEffect(() => {
    setPreviewUrl(null);
    loadPreview();
  }, [loadPreview]);

  // Videos: detect whether downloading is allowed. Probe /content with a
  // 1-byte range — share-link block-download policies reject that endpoint
  // with 403 even when the item metadata still lists a downloadUrl.
  useEffect(() => {
    let cancelled = false;
    setDownloadBlocked(false);
    if (!item || !isVideoFile(item)) return;
    probeDownloadAllowed(tab.driveId, item.id, tab.cloudEnv)
      .then((allowed) => {
        if (!cancelled) setDownloadBlocked(!allowed);
      })
      .catch(() => {
        // Probe failures keep the normal download path; it surfaces its own
        // errors if the file really is blocked.
      });
    return () => {
      cancelled = true;
    };
  }, [item, tab.driveId, tab.cloudEnv]);

  // ── Stream-capture download (in-webview pipeline) ──────────────────────────
  const handleStreamMessage = useCallback((event: MessageEvent) => {
    const data = event.data;
    if (!isSolMessage(data)) return;
    switch (data.type) {
      case "SOL_CAPTURED":
        if (data.captureId) setCapturedId(data.captureId);
        break;
      case "SOL_PROGRESS":
        setStreamProgress(data.text ?? "");
        break;
      case "SOL_DONE":
        setStreamState("done");
        setStreamProgress("");
        addToast("success", t("preview.streamDone"));
        break;
      case "SOL_ERROR":
        setStreamState("error");
        setStreamProgress("");
        setStreamErrorDetail(data.message ?? "");
        console.error("[stream-download] pipeline failed:", data);
        addToast(
          "error",
          data.isDrm ? t("preview.streamDrm") : `${t("preview.streamFailed")}: ${data.message ?? ""}`
        );
        break;
      default:
        break;
    }
  }, [addToast, t]);

  useEffect(() => {
    window.addEventListener("message", handleStreamMessage);
    return () => window.removeEventListener("message", handleStreamMessage);
  }, [handleStreamMessage]);

  // Abort the in-page pipeline when the tab unmounts mid-download.
  useEffect(() => {
    return () => {
      if (capturedId) {
        iframeRef.current?.contentWindow?.postMessage(
          { type: "SOL_CANCEL", captureId: capturedId },
          "*"
        );
      }
    };
  }, [capturedId]);

  const handleStreamDownload = useCallback(async () => {
    if (!item || !capturedId || streamState === "downloading") return;
    try {
      const dir = lastDownloadPath ?? (await downloadDir());
      const baseName = item.name.replace(/\.[^.]+$/, "") || "recording";
      const selected = await save({ defaultPath: await join(dir, `${baseName}.mp4`) });
      if (!selected) return;
      setLastDownloadPath(await dirname(selected));
      setStreamState("downloading");
      setStreamProgress(t("preview.streamPreparing"));
      setStreamErrorDetail("");
      const channel = await beginStreamDownload(selected);
      channelRef.current = channel;
      iframeRef.current?.contentWindow?.postMessage(
        {
          type: "SOL_START",
          captureId: capturedId,
          port: channel.port,
          uploadToken: channel.uploadToken,
          concurrency: segmentConcurrency,
        },
        "*"
      );
    } catch (err) {
      setStreamState("error");
      setStreamProgress("");
      addToast("error", getErrorMessage(err));
    }
  }, [item, capturedId, streamState, lastDownloadPath, setLastDownloadPath, segmentConcurrency, addToast, t]);

  const handleStreamCancel = useCallback(() => {
    if (capturedId) {
      iframeRef.current?.contentWindow?.postMessage(
        { type: "SOL_CANCEL", captureId: capturedId },
        "*"
      );
    }
    setStreamState("idle");
    setStreamProgress("");
  }, [capturedId]);

  const handleDownload = useCallback(async () => {
    if (!item || isDownloading) return;
    // No download permission (tenant policy rejects /content): the download
    // button routes to the stream-capture pipeline, after telling the user.
    if (downloadBlocked && isVideoFile(item) && isEmbed) {
      if (!capturedId) {
        addToast("info", t("preview.streamNeedCapture"));
        return;
      }
      addToast("info", t("preview.streamNotice"));
      await handleStreamDownload();
      return;
    }
    setIsDownloading(true);
    try {
      const dir = lastDownloadPath ?? (await downloadDir());
      const defaultPath = await join(dir, item.name);
      const selected = await save({ defaultPath });
      if (!selected) return;
      setLastDownloadPath(await dirname(selected));
      const batch = await downloadFile(
        tab.driveId,
        item.id,
        tab.homeAccountId,
        item.name,
        item.size ?? 0,
        selected,
        tab.cloudEnv
      );
      useTaskStore.getState().registerTask(batch.batchId, {
        type: "download",
        fileName: batch.batchName,
        homeAccountId: tab.homeAccountId,
        driveId: tab.driveId,
        cloudEnv: tab.cloudEnv,
        itemId: item.id,
        localPath: selected,
      });
    } catch (err) {
      addToast("error", getErrorMessage(err));
    } finally {
      setIsDownloading(false);
    }
  }, [
    item,
    isDownloading,
    isEmbed,
    downloadBlocked,
    capturedId,
    handleStreamDownload,
    lastDownloadPath,
    setLastDownloadPath,
    tab.driveId,
    tab.cloudEnv,
    addToast,
    t,
  ]);

  if (!item) return null;

  const isImage = isImageFile(item);
  const isVideo = isVideoFile(item);
  const isMarkdown = isMarkdownFile(item);
  // The stream pipeline rides on the SharePoint player inside the Graph
  // preview embed, so it only applies when that embed is what's on screen.
  const streamAvailable = isVideo && isEmbed;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center justify-between border-b border-border px-3 py-2">
        <span className="truncate text-sm font-medium text-foreground" title={item.name}>
          {item.name}
        </span>
        <div className="flex shrink-0 items-center gap-1">
          {streamAvailable && (
            <button
              onClick={handleStreamDownload}
              disabled={!capturedId || streamState === "downloading"}
              className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-foreground transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              title={capturedId ? t("preview.streamDownload") : t("preview.streamNeedCapture")}
              aria-label={t("preview.streamDownload")}
            >
              <Video className="h-3.5 w-3.5" />
            </button>
          )}
          <button
            onClick={handleDownload}
            disabled={isDownloading}
            className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-foreground transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            title={t("fileOps.download")}
            aria-label={t("fileOps.download")}
          >
            <Download className="h-3.5 w-3.5" />
          </button>
          {item.webUrl && (
            <a
              href={item.webUrl}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
              title={t("preview.openInBrowser")}
            >
              <ExternalLink className="h-3.5 w-3.5" />
            </a>
          )}
        </div>
      </div>

      {streamAvailable && streamState !== "idle" && (
        <div className="flex items-center gap-2 border-b border-border bg-muted/30 px-3 py-1.5 text-xs text-muted-foreground">
          {streamState === "downloading" && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          <span
            className="truncate"
            title={
              streamState === "error" && streamErrorDetail
                ? `${t("preview.streamFailed")}: ${streamErrorDetail}`
                : streamState === "downloading"
                  ? streamProgress
                  : undefined
            }
          >
            {streamState === "downloading" && streamProgress}
            {streamState === "done" && t("preview.streamDone")}
            {streamState === "error" &&
              (streamErrorDetail
                ? `${t("preview.streamFailed")}: ${streamErrorDetail}`
                : t("preview.streamFailed"))}
          </span>
          {streamState === "downloading" && (
            <>
              <span className="ml-auto shrink-0 whitespace-nowrap text-muted-foreground/80">
                {t("preview.streamKeepOpen")}
              </span>
              <button
                onClick={handleStreamCancel}
                className="shrink-0 rounded-md px-2 py-0.5 hover:bg-accent hover:text-foreground transition-colors"
              >
                {t("dialogs.cancel")}
              </button>
            </>
          )}
        </div>
      )}
      {streamAvailable && streamState === "idle" && !capturedId && (
        <div className="border-b border-border bg-muted/20 px-3 py-1.5 text-xs text-muted-foreground">
          {t("preview.streamNeedCapture")}
        </div>
      )}

      <div className="flex flex-1 min-h-0 items-center justify-center overflow-auto bg-muted/20">
        {isLoading && (
          <div className="flex flex-col items-center gap-3 p-8">
            <Loader2 className="h-7 w-7 animate-spin text-muted-foreground" />
            <p className="text-sm text-muted-foreground">{t("preview.loading")}</p>
          </div>
        )}

        {!isLoading && error && (
          <div className="flex flex-col items-center gap-3 p-8">
            <AlertCircle className="h-8 w-8 text-destructive" />
            <p className="text-sm text-muted-foreground">{error}</p>
            <button
              onClick={loadPreview}
              className="inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
            >
              <RotateCw className="h-3.5 w-3.5" />
              {t("preview.retry")}
            </button>
          </div>
        )}

        {!isLoading && !error && textContent !== null && isMarkdown && (
          <div className="h-full w-full overflow-auto bg-background p-6">
            <div className="markdown-preview mx-auto max-w-3xl">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{textContent}</ReactMarkdown>
            </div>
          </div>
        )}

        {!isLoading && !error && textContent !== null && !isMarkdown && (
          <pre className="h-full w-full overflow-auto whitespace-pre-wrap p-4 font-mono text-xs text-foreground">
            {textContent}
          </pre>
        )}

        {!isLoading && !error && previewUrl && isImage && (
          <img
            src={previewUrl}
            alt={item.name}
            className="max-h-full max-w-full object-contain"
            onError={() => setError(t("preview.loadFailed"))}
          />
        )}

        {!isLoading && !error && previewUrl && isVideo && !isEmbed && (
          <video
            src={previewUrl}
            controls
            autoPlay
            className="max-h-full max-w-full"
            onError={() => setError(t("preview.loadFailed"))}
          >
            {t("preview.loadFailed")}
          </video>
        )}

        {!isLoading && !error && previewUrl && !isImage && !(isVideo && !isEmbed) && (
          <iframe
            ref={iframeRef}
            src={previewUrl}
            title={item.name}
            className="h-full w-full border-0 bg-white"
            allow="autoplay; fullscreen; encrypted-media; picture-in-picture"
          />
        )}
      </div>
    </div>
  );
}
