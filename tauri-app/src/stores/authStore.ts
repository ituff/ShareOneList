import { create } from "zustand";
import type { AccountEntry, AccountInfo, CloudEnvironment } from "../lib/types";
import { getAccounts, login, logout } from "../lib/tauri";

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

  clearError: () => set({ error: null }),

  setPendingRelogin: (pending) => set({ pendingRelogin: pending }),

  clearPendingRelogin: () => set({ pendingRelogin: null }),

  reloginAccount: async (cloudEnv) => {
    set({ isLoggingIn: true, error: null });
    try {
      const accountInfo = await login(cloudEnv);
      const newEntry: AccountEntry = {
        homeAccountId: accountInfo.homeAccountId,
        driveId: accountInfo.driveId,
        cloudType: accountInfo.cloudEnv,
        displayName: accountInfo.displayName,
      };
      set((state) => ({
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
      }));
    } catch (err) {
      const message = err instanceof Error ? err.message : "Login failed";
      set({ isLoggingIn: false, error: message });
      throw err;
    }
  },
}));
