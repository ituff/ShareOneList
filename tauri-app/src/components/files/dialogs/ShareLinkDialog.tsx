import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Copy, Check } from "lucide-react";
import type { CloudEnvironment, DriveItem, ShareOptions } from "../../../lib/types";
import { createShareLink } from "../../../lib/tauri";
import { useToastStore } from "../../../stores/toastStore";

interface ShareLinkDialogProps {
  item: DriveItem;
  cloudEnv: CloudEnvironment;
  driveId: string;
  onClose: () => void;
}

/**
 * Modal dialog for creating a sharing link for a file or folder.
 * Supports view/edit link type, optional expiration date, and optional password.
 * Displays the generated link with a copy-to-clipboard button on success.
 */
export function ShareLinkDialog({ item, cloudEnv, driveId, onClose }: ShareLinkDialogProps) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const [linkType, setLinkType] = useState<"view" | "edit">("view");
  const [expiration, setExpiration] = useState("");
  const [password, setPassword] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [shareUrl, setShareUrl] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !isLoading) onClose();
    };
    document.addEventListener("keydown", handleEscape);
    return () => document.removeEventListener("keydown", handleEscape);
  }, [isLoading, onClose]);

  const handleCreateLink = async () => {
    setIsLoading(true);
    setError(null);
    try {
      const options: ShareOptions = {
        linkType,
        expiration: expiration || null,
        password: password || null,
      };
      const url = await createShareLink(driveId, item.id, options, cloudEnv);
      setShareUrl(url);
    } catch (err) {
      const message = err instanceof Error ? err.message : t("errors.unknownError");
      setError(message);
      addToast("error", message);
    } finally {
      setIsLoading(false);
    }
  };

  const handleCopy = async () => {
    if (!shareUrl) return;
    try {
      await navigator.clipboard.writeText(shareUrl);
      setCopied(true);
      addToast("success", t("dialogs.share.linkCopied"));
      setTimeout(() => setCopied(false), 2000);
    } catch {
      addToast("error", t("errors.unknownError"));
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-labelledby="share-dialog-title"
    >
      <div
        className="w-full max-w-md rounded-lg bg-card p-6 shadow-lg border border-border"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="share-dialog-title" className="text-lg font-semibold text-foreground mb-4">
          {t("dialogs.share.title")}
        </h3>

        {!shareUrl ? (
          <>
            {/* Link type selector */}
            <div className="mb-4">
              <label className="text-sm font-medium text-foreground block mb-2">
                {t("dialogs.share.linkType")}
              </label>
              <div className="flex gap-2">
                <label
                  className={`flex-1 flex items-center justify-center gap-2 rounded-md border px-3 py-2 cursor-pointer text-sm transition-colors ${
                    linkType === "view"
                      ? "border-primary bg-primary/5 text-foreground"
                      : "border-border text-muted-foreground hover:bg-accent/30"
                  }`}
                >
                  <input
                    type="radio"
                    name="linkType"
                    value="view"
                    checked={linkType === "view"}
                    onChange={() => setLinkType("view")}
                    className="sr-only"
                  />
                  {t("dialogs.share.view")}
                </label>
                <label
                  className={`flex-1 flex items-center justify-center gap-2 rounded-md border px-3 py-2 cursor-pointer text-sm transition-colors ${
                    linkType === "edit"
                      ? "border-primary bg-primary/5 text-foreground"
                      : "border-border text-muted-foreground hover:bg-accent/30"
                  }`}
                >
                  <input
                    type="radio"
                    name="linkType"
                    value="edit"
                    checked={linkType === "edit"}
                    onChange={() => setLinkType("edit")}
                    className="sr-only"
                  />
                  {t("dialogs.share.edit")}
                </label>
              </div>
            </div>

            {/* Expiration date */}
            <div className="mb-4">
              <label className="text-sm font-medium text-foreground block mb-1">
                {t("dialogs.share.expiration")}
              </label>
              <input
                type="date"
                value={expiration}
                onChange={(e) => setExpiration(e.target.value)}
                className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground focus:outline-none focus:ring-2 focus:ring-ring"
                disabled={isLoading}
                aria-label={t("dialogs.share.expiration")}
              />
            </div>

            {/* Password */}
            <div className="mb-4">
              <label className="text-sm font-medium text-foreground block mb-1">
                {t("dialogs.share.password")}
              </label>
              <input
                type="text"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={t("dialogs.share.password")}
                className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring"
                disabled={isLoading}
                aria-label={t("dialogs.share.password")}
              />
            </div>

            {error && (
              <div className="mb-4 rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">
                {error}
              </div>
            )}

            <div className="flex justify-end gap-2">
              <button
                onClick={onClose}
                disabled={isLoading}
                className="rounded-md px-4 py-2 text-sm font-medium text-muted-foreground hover:bg-accent transition-colors disabled:opacity-50"
              >
                {t("dialogs.cancel")}
              </button>
              <button
                onClick={handleCreateLink}
                disabled={isLoading}
                className="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
              >
                {isLoading && <Loader2 className="h-4 w-4 animate-spin" />}
                {t("dialogs.share.title")}
              </button>
            </div>
          </>
        ) : (
          <>
            {/* Success: show the share link */}
            <div className="mb-4">
              <div className="flex items-center gap-2 rounded-md border border-border bg-background px-3 py-2">
                <input
                  type="text"
                  value={shareUrl}
                  readOnly
                  className="flex-1 bg-transparent text-sm text-foreground outline-none"
                  aria-label="Share link URL"
                />
                <button
                  onClick={handleCopy}
                  className="inline-flex items-center gap-1 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
                  aria-label={t("dialogs.share.copyLink")}
                >
                  {copied ? (
                    <Check className="h-3.5 w-3.5" />
                  ) : (
                    <Copy className="h-3.5 w-3.5" />
                  )}
                  {t("dialogs.share.copyLink")}
                </button>
              </div>
            </div>

            <div className="flex justify-end">
              <button
                onClick={onClose}
                className="rounded-md px-4 py-2 text-sm font-medium text-muted-foreground hover:bg-accent transition-colors"
              >
                {t("dialogs.close")}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
