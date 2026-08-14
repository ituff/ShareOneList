import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertCircle, Download, ExternalLink, Loader2, RotateCw } from "lucide-react";
import { dirname, downloadDir, join } from "@tauri-apps/api/path";
import { save } from "@tauri-apps/plugin-dialog";
import { downloadFile, getPreviewUrl, getTextContent } from "../../lib/tauri";
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

/** Full-tab file preview backed by the Graph preview API. */
export function PreviewPage({ tab }: PreviewPageProps) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const lastDownloadPath = useSettingsStore((s) => s.lastDownloadPath);
  const setLastDownloadPath = useSettingsStore((s) => s.setLastDownloadPath);
  const item = tab.previewItem;
  const [previewUrl, setPreviewUrl] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isEmbed, setIsEmbed] = useState(false);
  const [isDownloading, setIsDownloading] = useState(false);
  const [textContent, setTextContent] = useState<string | null>(null);

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

  const handleDownload = useCallback(async () => {
    if (!item || isDownloading) return;
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
  }, [item, isDownloading, lastDownloadPath, setLastDownloadPath, tab.driveId, tab.cloudEnv, addToast]);

  if (!item) return null;

  const isImage = isImageFile(item);
  const isVideo = isVideoFile(item);
  const isMarkdown = isMarkdownFile(item);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center justify-between border-b border-border px-3 py-2">
        <span className="truncate text-sm font-medium text-foreground" title={item.name}>
          {item.name}
        </span>
        <div className="flex shrink-0 items-center gap-1">
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
