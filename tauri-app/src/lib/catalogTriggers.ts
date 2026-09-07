import type { AccountEntry } from "./types";
import { catalogRegisterDrive } from "./tauri";

/**
 * Fire-and-forget catalog registration of an account's OneDrive.
 * Called when accounts load (restored sessions) and after each login.
 * Failures are swallowed: the catalog is auxiliary data.
 */
export function registerAccountOneDrives(accounts: AccountEntry[]): void {
  for (const account of accounts) {
    catalogRegisterDrive({
      accountId: account.homeAccountId,
      cloudEnv: account.cloudType,
      driveId: account.driveId,
      kind: "onedrive",
      name: account.alias || account.displayName,
      siteName: "",
    }).catch(() => undefined);
  }
}
