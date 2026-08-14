import { useTranslation } from "react-i18next";
import { HardDrive, Building2, Users, ArrowLeft } from "lucide-react";
import type { AccountEntry, CloudEnvironment } from "../../lib/types";
import { StorageInfo } from "./StorageInfo";
import { useToastStore } from "../../stores/toastStore";

interface DriveHubPageProps {
  account: AccountEntry;
  onDriveSelect: (
    driveId: string,
    driveName: string,
    cloudEnv: CloudEnvironment,
    homeAccountId: string
  ) => void;
  onSharePointSelect: () => void;
  onSharedSelect: () => void;
  onBack: () => void;
}

/**
 * Service selection page shown after selecting an account.
 * Lets the user choose between OneDrive, SharePoint, or Shared drives.
 */
export function DriveHubPage({
  account,
  onDriveSelect,
  onSharePointSelect,
  onSharedSelect,
  onBack,
}: DriveHubPageProps) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);

  const handleOneDrive = () => {
    if (!account.driveId) {
      addToast("error", t("driveHub.noOneDriveDesc"));
      return;
    }
    onDriveSelect(account.driveId, account.displayName, account.cloudType, account.homeAccountId);
  };

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
          {account.displayName === "Unknown User"
            ? account.cloudType.toLowerCase() === "global"
              ? t("accounts.cloudGlobal")
              : t("accounts.cloudChina")
            : account.displayName}
        </h3>
      </div>

      {/* Service selection cards */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
        {/* OneDrive */}
        <button
          onClick={handleOneDrive}
          className="flex flex-col items-center gap-3 rounded-lg border border-border bg-card p-6 hover:bg-accent/30 hover:border-primary/50 transition-colors cursor-pointer"
        >
          <HardDrive className="h-10 w-10 text-blue-500" />
          <span className="text-sm font-medium text-foreground">
            {t("driveHub.oneDrive")}
          </span>
        </button>

        {/* SharePoint */}
        <button
          onClick={onSharePointSelect}
          className="flex flex-col items-center gap-3 rounded-lg border border-border bg-card p-6 hover:bg-accent/30 hover:border-primary/50 transition-colors cursor-pointer"
        >
          <Building2 className="h-10 w-10 text-purple-500" />
          <span className="text-sm font-medium text-foreground">
            {t("driveHub.sharePoint")}
          </span>
        </button>

        {/* Shared with me */}
        <button
          onClick={onSharedSelect}
          className="flex flex-col items-center gap-3 rounded-lg border border-border bg-card p-6 hover:bg-accent/30 hover:border-primary/50 transition-colors cursor-pointer"
        >
          <Users className="h-10 w-10 text-green-500" />
          <span className="text-sm font-medium text-foreground">
            {t("driveHub.sharedWithMe")}
          </span>
        </button>
      </div>

      {/* Storage quota display */}
      <StorageInfo driveId={account.driveId} cloudEnv={account.cloudType} />
    </div>
  );
}
