import { useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { useToastStore } from "../../stores/toastStore";

type DownloaderType = "Aria2" | "Motrix" | "Idm";

const DEFAULT_ENDPOINTS: Record<DownloaderType, string> = {
  Aria2: "http://localhost:6800/jsonrpc",
  Motrix: "http://localhost:16800/jsonrpc",
  Idm: "",
};

export function ToolsPage() {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);

  // SharePoint URL parsing state
  const [shareUrl, setShareUrl] = useState("");
  const [parsedUrl, setParsedUrl] = useState<string | null>(null);
  const [parseError, setParseError] = useState<string | null>(null);

  // Downloader configuration state
  const [downloaderType, setDownloaderType] = useState<DownloaderType>("Aria2");
  const [rpcUrl, setRpcUrl] = useState(DEFAULT_ENDPOINTS.Aria2);
  const [rpcSecret, setRpcSecret] = useState("");
  const [fileName, setFileName] = useState("");
  const [pushing, setPushing] = useState(false);

  const handleParse = async () => {
    setParseError(null);
    setParsedUrl(null);

    if (!shareUrl.trim()) {
      setParseError(t("errors.cannotBeEmpty"));
      return;
    }

    try {
      const result = await invoke<string | null>("parse_sharepoint_url", {
        url: shareUrl.trim(),
      });

      if (result) {
        setParsedUrl(result);
        // Auto-extract filename from URL if possible
        try {
          const urlObj = new URL(shareUrl.trim());
          const pathParts = urlObj.pathname.split("/").filter(Boolean);
          if (pathParts.length > 0) {
            const lastPart = pathParts[pathParts.length - 1];
            if (lastPart && !lastPart.startsWith("_") && lastPart.includes(".")) {
              setFileName(decodeURIComponent(lastPart));
            }
          }
        } catch {
          // Ignore URL parsing errors for filename extraction
        }
      } else {
        setParseError(t("errors.parameterError"));
      }
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      setParseError(message);
    }
  };

  const handleDownloaderChange = (type: DownloaderType) => {
    setDownloaderType(type);
    setRpcUrl(DEFAULT_ENDPOINTS[type]);
  };

  const handleCopyLink = async () => {
    if (parsedUrl) {
      await navigator.clipboard.writeText(parsedUrl);
      addToast("success", t("success.linkCopied"));
    }
  };

  const handlePushDownload = async () => {
    if (!parsedUrl) return;

    setPushing(true);
    try {
      await invoke("push_to_downloader", {
        config: {
          downloader_type: downloaderType,
          rpc_url: rpcUrl,
          secret: rpcSecret || null,
          download_url: parsedUrl,
          file_name: fileName || "download",
        },
      });
      addToast("success", t("success.success"));
    } catch (err: unknown) {
      const message =
        err && typeof err === "object" && "message" in err
          ? (err as { message: string }).message
          : String(err);
      addToast("error", message);
    } finally {
      setPushing(false);
    }
  };

  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <h2 className="text-2xl font-bold text-foreground">{t("tools.title")}</h2>
        <p className="text-muted-foreground">{t("tools.description")}</p>
      </div>

      {/* SharePoint URL Parser Section */}
      <section className="space-y-3 p-4 rounded-lg border border-border bg-card">
        <h3 className="text-lg font-semibold text-foreground">
          {t("tools.shareLink")}
        </h3>

        <div className="flex gap-2">
          <input
            type="text"
            value={shareUrl}
            onChange={(e) => setShareUrl(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleParse()}
            placeholder="https://xxx.sharepoint.com/..."
            className="flex-1 px-3 py-2 rounded-md border border-input bg-background text-foreground text-sm placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring"
          />
          <button
            onClick={handleParse}
            className="px-4 py-2 rounded-md bg-primary text-primary-foreground text-sm font-medium hover:bg-primary/90 transition-colors"
          >
            {t("tools.parseLink")}
          </button>
        </div>

        {/* Parse Result */}
        {parsedUrl && (
          <div className="space-y-2 p-3 rounded-md bg-muted/50">
            <p className="text-sm font-medium text-foreground">{t("tools.parseResult")}</p>
            <div className="flex items-center gap-2">
              <code className="flex-1 text-xs text-muted-foreground break-all select-all">
                {parsedUrl}
              </code>
              <button
                onClick={handleCopyLink}
                className="shrink-0 px-3 py-1 rounded-md border border-input bg-background text-foreground text-xs hover:bg-accent transition-colors"
              >
                {t("tools.copyLink")}
              </button>
            </div>
          </div>
        )}

        {/* Parse Error */}
        {parseError && (
          <p className="text-sm text-destructive">{parseError}</p>
        )}
      </section>

      {/* Downloader Configuration Section */}
      <section className="space-y-4 p-4 rounded-lg border border-border bg-card">
        <h3 className="text-lg font-semibold text-foreground">
          {t("tools.downloadTool")}
        </h3>

        {/* Downloader Type Selector */}
        <div className="space-y-2">
          <label className="text-sm font-medium text-foreground">
            {t("tools.selectDownloader")}
          </label>
          <div className="flex gap-2">
            {(["Aria2", "Motrix", "Idm"] as DownloaderType[]).map((type) => (
              <button
                key={type}
                onClick={() => handleDownloaderChange(type)}
                className={`px-4 py-2 rounded-md text-sm font-medium transition-colors ${
                  downloaderType === type
                    ? "bg-primary text-primary-foreground"
                    : "bg-muted text-muted-foreground hover:bg-accent hover:text-foreground"
                }`}
              >
                {type === "Idm" ? "IDM" : type}
              </button>
            ))}
          </div>
        </div>

        {/* RPC/Path Configuration */}
        <div className="space-y-3">
          <div className="space-y-1">
            <label className="text-sm font-medium text-foreground">
              {downloaderType === "Idm" ? t("tools.idmPath") : t("tools.rpcAddress")}
            </label>
            <input
              type="text"
              value={rpcUrl}
              onChange={(e) => setRpcUrl(e.target.value)}
              placeholder={
                downloaderType === "Idm"
                  ? "C:\\Program Files\\Internet Download Manager\\IDMan.exe"
                  : DEFAULT_ENDPOINTS[downloaderType]
              }
              className="w-full px-3 py-2 rounded-md border border-input bg-background text-foreground text-sm placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring"
            />
          </div>

          {/* RPC Secret (only for Aria2/Motrix) */}
          {downloaderType !== "Idm" && (
            <div className="space-y-1">
              <label className="text-sm font-medium text-foreground">
                {t("tools.rpcSecret")}
              </label>
              <input
                type="password"
                value={rpcSecret}
                onChange={(e) => setRpcSecret(e.target.value)}
                placeholder="(optional)"
                className="w-full px-3 py-2 rounded-md border border-input bg-background text-foreground text-sm placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring"
              />
            </div>
          )}
        </div>

        {/* Push Download Button */}
        <button
          onClick={handlePushDownload}
          disabled={!parsedUrl || pushing}
          className="px-4 py-2 rounded-md bg-primary text-primary-foreground text-sm font-medium hover:bg-primary/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          title={!parsedUrl ? t("tools.selectDownloaderTooltip") : undefined}
        >
          {pushing ? "..." : t("tools.pushDownload")}
        </button>
      </section>
    </div>
  );
}
