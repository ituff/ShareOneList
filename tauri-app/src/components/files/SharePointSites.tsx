import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowLeft, Building2, RefreshCw, AlertCircle } from "lucide-react";
import { getSharepointSites } from "../../lib/tauri";
import { getErrorMessage } from "../../lib/errors";
import type { CloudEnvironment, Site } from "../../lib/types";

interface SharePointSitesProps {
  cloudEnv: CloudEnvironment;
  homeAccountId: string;
  onSiteSelect: (site: Site) => void;
  onBack: () => void;
}

/**
 * Lists available SharePoint sites for the current account.
 * Handles loading, empty, and error states with retry support.
 */
export function SharePointSites({ cloudEnv, homeAccountId, onSiteSelect, onBack }: SharePointSitesProps) {
  const { t } = useTranslation();
  const [sites, setSites] = useState<Site[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchSites = async () => {
    setIsLoading(true);
    setError(null);
    try {
      const result = await getSharepointSites(cloudEnv, homeAccountId);
      setSites(result);
    } catch (err) {
      setError(getErrorMessage(err));
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    fetchSites();
  }, [cloudEnv, homeAccountId]);

  return (
    <div className="space-y-4">
      {/* Header with back button */}
      <div className="flex items-center gap-3">
        <button
          onClick={onBack}
          className="rounded-md p-1.5 text-muted-foreground hover:bg-accent transition-colors"
          aria-label={t("files.back")}
        >
          <ArrowLeft className="h-5 w-5" />
        </button>
        <h3 className="text-lg font-semibold text-foreground">
          {t("driveHub.sharePoint")}
        </h3>
      </div>

      {/* Loading state */}
      {isLoading && (
        <div className="flex items-center justify-center py-12">
          <RefreshCw className="h-5 w-5 animate-spin text-muted-foreground" />
        </div>
      )}

      {/* Error state */}
      {!isLoading && error && (
        <div className="rounded-lg border border-destructive/50 bg-destructive/10 p-6 text-center space-y-3">
          <AlertCircle className="h-8 w-8 text-destructive mx-auto" />
          <p className="text-sm text-destructive">{error}</p>
          <button
            onClick={fetchSites}
            className="inline-flex items-center gap-2 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
          >
            <RefreshCw className="h-4 w-4" />
            {t("errors.retryAction")}
          </button>
        </div>
      )}

      {/* Empty state */}
      {!isLoading && !error && sites.length === 0 && (
        <div className="rounded-lg border border-dashed border-border p-8 text-center space-y-2">
          <Building2 className="h-8 w-8 text-muted-foreground mx-auto" />
          <p className="text-sm font-medium text-foreground">
            {t("driveHub.noSharePoint")}
          </p>
          <p className="text-xs text-muted-foreground">
            {t("driveHub.noSharePointDesc")}
          </p>
        </div>
      )}

      {/* Sites list */}
      {!isLoading && !error && sites.length > 0 && (
        <div className="space-y-2">
          {sites.map((site) => (
            <button
              key={site.id}
              onClick={() => onSiteSelect(site)}
              className="w-full flex items-center gap-3 rounded-lg border border-border bg-card px-4 py-3 hover:bg-accent/30 transition-colors cursor-pointer text-left"
            >
              <Building2 className="h-5 w-5 text-purple-500 shrink-0" />
              <div className="min-w-0">
                <div className="text-sm font-medium text-foreground truncate">
                  {site.displayName}
                </div>
                <div className="text-xs text-muted-foreground truncate">
                  {site.webUrl}
                </div>
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
