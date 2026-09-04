import { useTranslation } from "react-i18next";
import { HardDrive, Building2, Video, ArrowLeft } from "lucide-react";
import type { AccountEntry, CloudEnvironment } from "../../lib/types";
import {
  accountDisplayName,
  accountKindLabelKey,
  resolveAccountKind,
  supportsMeetingRecordings,
  supportsSharePoint,
} from "../../lib/account";
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
  onMeetingsSelect: () => void;
  onBack: () => void;
}

/**
 * Service selection page shown after selecting an account.
 * The offered services depend on the account kind:
 * - 21Vianet: OneDrive, SharePoint
 * - Global organization: OneDrive, SharePoint, Meeting recordings
 * - Global personal: OneDrive
 */
export function DriveHubPage({
  account,
  onDriveSelect,
  onSharePointSelect,
  onMeetingsSelect,
  onBack,
}: DriveHubPageProps) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const kind = resolveAccountKind(account.cloudType, account.accountType ?? null);
  const showSharePoint = supportsSharePoint(kind);
  const showRecordings = supportsMeetingRecordings(kind);

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
          {accountDisplayName(account) || t(accountKindLabelKey(kind))}
        </h3>
        <span className="rounded-full border border-border px-2 py-0.5 text-xs text-muted-foreground">
          {t(accountKindLabelKey(kind))}
        </span>
      </div>

      {/* Service selection cards */}
      <div className="grid grid-cols-2 sm:grid-cols-3 gap-4">
        {/* OneDrive (all account kinds) */}
        <button
          onClick={handleOneDrive}
          className="flex flex-col items-center gap-3 rounded-lg border border-border bg-card p-6 hover:bg-accent/30 hover:border-primary/50 transition-colors cursor-pointer"
        >
          <HardDrive className="h-10 w-10 text-blue-500" />
          <span className="text-sm font-medium text-foreground">
            {t("driveHub.oneDrive")}
          </span>
        </button>

        {/* SharePoint (21Vianet and global organization accounts) */}
        {showSharePoint && (
          <button
            onClick={onSharePointSelect}
            className="flex flex-col items-center gap-3 rounded-lg border border-border bg-card p-6 hover:bg-accent/30 hover:border-primary/50 transition-colors cursor-pointer"
          >
            <Building2 className="h-10 w-10 text-purple-500" />
            <span className="text-sm font-medium text-foreground">
              {t("driveHub.sharePoint")}
            </span>
          </button>
        )}

        {/* Meeting recordings (global organization accounts only) */}
        {showRecordings && (
          <button
            onClick={onMeetingsSelect}
            className="flex flex-col items-center gap-3 rounded-lg border border-border bg-card p-6 hover:bg-accent/30 hover:border-primary/50 transition-colors cursor-pointer"
          >
            <Video className="h-10 w-10 text-red-500" />
            <span className="text-sm font-medium text-foreground">
              {t("driveHub.meetingRecordings")}
            </span>
          </button>
        )}
      </div>

      {/* Storage quota display */}
      <StorageInfo driveId={account.driveId} cloudEnv={account.cloudType} />
    </div>
  );
}
