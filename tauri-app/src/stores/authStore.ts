import { create } from "zustand";
import type { AccountEntry, AccountInfo, CloudEnvironment } from "../lib/types";
import {
  getAccounts,
  login,
  logout,
  refreshAccountType,
  updateAccount,
} from "../lib/tauri";

interface AuthState {
  /** List of connected accounts. */
  accounts: AccountEntry[];
  /** Whether accounts have been loaded from the backend. */
  isLoaded: boolean;
  /** Whether a login operation is in progress. */
  isLoggingIn: boolean;
  /** Error message from the last failed operation. */
  error: string | null;
  /** Pending re-login requested after an expired credential error. */
  pendingRelogin:
    | { cloudEnv: CloudEnvironment; tabId?: string; folderId?: string; taskId?: string }
    | null;

  /** Load cached accounts from backend on startup. */
  loadAccounts: () => Promise<void>;
  /**
   * Add a new account by initiating the OAuth2 login flow.
   * Returns the new account info on success, or throws on failure.
   * Rejects if the home_account_id already exists (duplicate detection).
   */
  addAccount: (cloudEnv: CloudEnvironment, displayName?: string) => Promise<AccountInfo>;
  /**
   * Remove an account: call logout on backend and remove from local state.
   */
  removeAccount: (homeAccountId: string, cloudEnv: CloudEnvironment) => Promise<void>;
  /** Persist a new alias/icon for an account and update local state. */
  changeAccount: (
    homeAccountId: string,
    cloudEnv: CloudEnvironment,
    alias?: string | null,
    icon?: string | null
  ) => Promise<void>;
  /**
   * Re-derive each account's personal/organization type from the backend and
   * update local state. Heals entries saved before driveType-based detection.
   */
  refreshAccountTypes: () => Promise<void>;
  /** Clear the current error. */
  clearError: () => void;
  /** Queue a re-login flow after an expired credential error. */
  setPendingRelogin: (
    pending: { cloudEnv: CloudEnvironment; tabId?: string; folderId?: string; taskId?: string }
  ) => void;
  /** Clear the pending re-login request. */
  clearPendingRelogin: () => void;
  /** Re-login an existing account and update its stored entry. */
  reloginAccount: (cloudEnv: CloudEnvironment) => Promise<void>;
}

export const useAuthStore = create<AuthState>((set, get) => ({
  accounts: [],
  isLoaded: false,
  isLoggingIn: false,
  error: null,
  pendingRelogin: null,

  loadAccounts: async () => {
    try {
      const accounts = await getAccounts();
      // Older builds persisted cloudType as "Global"/"China"; normalize it here.
      const normalized = accounts.map((account) => ({
        ...account,
        cloudType: account.cloudType.toLowerCase() as CloudEnvironment,
      }));
      set({ accounts: normalized, isLoaded: true });
    } catch (err) {
      console.error("[authStore] Failed to load accounts:", err);
      // Mark as loaded even on failure so the app can proceed
      set({ isLoaded: true });
    }
  },

  addAccount: async (cloudEnv, displayName) => {
    set({ isLoggingIn: true, error: null });
    try {
      const accountInfo: AccountInfo = await login(cloudEnv);

      const { accounts } = get();

      // Duplicate detection: reject if home_account_id already exists
      const duplicate = accounts.find(
        (a) => a.homeAccountId === accountInfo.homeAccountId
      );
      if (duplicate) {
        const error = "DUPLICATE_ACCOUNT";
        set({ isLoggingIn: false, error });
        throw new Error(error);
      }

      // Create the new account entry
      const newEntry: AccountEntry = {
        homeAccountId: accountInfo.homeAccountId,
        driveId: accountInfo.driveId,
        cloudType: accountInfo.cloudEnv,
        displayName: displayName || accountInfo.displayName,
        accountType: accountInfo.accountType ?? null,
      };

      set({
        accounts: [...accounts, newEntry],
        isLoggingIn: false,
        error: null,
      });

      return accountInfo;
    } catch (err) {
      const message =
        err instanceof Error ? err.message : "Login failed";
      set({ isLoggingIn: false, error: message });
      throw err;
    }
  },

  removeAccount: async (homeAccountId, cloudEnv) => {
    try {
      await logout(cloudEnv, homeAccountId);
    } catch (err) {
      console.error("[authStore] Logout backend call failed:", err);
      // Continue with removal even if backend logout fails
    }

    set((state) => ({
      accounts: state.accounts.filter(
        (a) => a.homeAccountId !== homeAccountId
      ),
    }));
  },

  changeAccount: async (homeAccountId, cloudEnv, alias, icon) => {
    const updated = await updateAccount(cloudEnv, homeAccountId, alias, icon);
    set((state) => ({
      accounts: state.accounts.map((account) =>
        account.homeAccountId === homeAccountId && account.cloudType === cloudEnv
          ? {
              ...account,
              alias: updated.alias ?? null,
              icon: updated.icon ?? null,
            }
          : account
      ),
    }));
  },

  refreshAccountTypes: async () => {
    const { accounts } = get();
    // Re-derive each account's type in the background; a failure (e.g. an
    // expired session raising the relogin dialog) leaves the entry untouched.
    await Promise.all(
      accounts.map(async (account) => {
        try {
          const resolved = await refreshAccountType(
            account.cloudType,
            account.homeAccountId
          );
          if (resolved !== "personal" && resolved !== "organization") return;
          set((state) => ({
            accounts: state.accounts.map((entry) =>
              entry.homeAccountId === account.homeAccountId &&
              entry.cloudType === account.cloudType
                ? { ...entry, accountType: resolved }
                : entry
            ),
          }));
        } catch (err) {
          console.error(
            `[authStore] Failed to refresh account type for ${account.homeAccountId}:`,
            err
          );
        }
      })
    );
  },

  clearError: () => set({ error: null }),

  setPendingRelogin: (pending) => set({ pendingRelogin: pending }),

  clearPendingRelogin: () => set({ pendingRelogin: null }),

  reloginAccount: async (cloudEnv) => {
    set({ isLoggingIn: true, error: null });
    try {
      const accountInfo = await login(cloudEnv);
      set((state) => {
        // Carry over the user-customized alias/icon from the existing entry.
        const previous = state.accounts.find(
          (account) => account.homeAccountId === accountInfo.homeAccountId
        );
        const newEntry: AccountEntry = {
          homeAccountId: accountInfo.homeAccountId,
          driveId: accountInfo.driveId,
          cloudType: accountInfo.cloudEnv,
          displayName: accountInfo.displayName,
          accountType: accountInfo.accountType ?? null,
          alias: previous?.alias ?? null,
          icon: previous?.icon ?? null,
        };
        return {
          accounts: [
            ...state.accounts.filter(
              (account) =>
                !(
                  account.homeAccountId === newEntry.homeAccountId &&
                  account.cloudType === newEntry.cloudType
                )
            ),
            newEntry,
          ],
          isLoggingIn: false,
          error: null,
        };
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : "Login failed";
      set({ isLoggingIn: false, error: message });
      throw err;
    }
  },
}));
