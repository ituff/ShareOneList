import {
  Briefcase,
  Camera,
  Cloud,
  Globe,
  Heart,
  Leaf,
  Music,
  Rocket,
  Star,
  Sun,
  UserRound,
  type LucideIcon,
} from "lucide-react";
import type { AccountType, CloudEnvironment } from "./types";

/**
 * The user-facing account kinds. The app distinguishes three real kinds —
 * 21Vianet, global organization, and global personal — because they expose
 * different services and different Graph endpoints. `global-legacy` covers
 * accounts persisted before the accountType field existed.
 */
export type AccountKind =
  | "china"
  | "global-org"
  | "global-personal"
  | "global-legacy";

/** Resolve the account kind from its cloud environment and account type. */
export function resolveAccountKind(
  cloudEnv: CloudEnvironment,
  accountType?: AccountType | null
): AccountKind {
  if (cloudEnv === "china") return "china";
  if (accountType === "personal") return "global-personal";
  if (accountType === "organization") return "global-org";
  return "global-legacy";
}

/** i18n key of the display label for an account kind. */
export function accountKindLabelKey(kind: AccountKind): string {
  switch (kind) {
    case "china":
      return "accounts.cloudChina";
    case "global-org":
      return "accounts.cloudGlobalOrganization";
    case "global-personal":
      return "accounts.cloudGlobalPersonal";
    case "global-legacy":
      return "accounts.cloudGlobal";
  }
}

/** Whether the account can see SharePoint sites. */
export function supportsSharePoint(kind: AccountKind): boolean {
  return kind === "china" || kind === "global-org" || kind === "global-legacy";
}

/** Whether the account can list Teams meeting recordings. */
export function supportsMeetingRecordings(kind: AccountKind): boolean {
  return kind === "global-org" || kind === "global-legacy";
}

// ─── Display name & icon library ────────────────────────────────────────────

/** The name shown for an account: user alias first, then display name. */
export function accountDisplayName(account: {
  displayName: string;
  alias?: string | null;
}): string {
  if (account.alias && account.alias.trim()) return account.alias.trim();
  if (account.displayName && account.displayName !== "Unknown User") {
    return account.displayName;
  }
  return "";
}

/** One selectable icon in the built-in account icon library. */
export interface AccountIconDef {
  id: string;
  icon: LucideIcon;
  className: string;
}

/** Default icon library offered when personalizing an account. */
export const ACCOUNT_ICON_LIBRARY: AccountIconDef[] = [
  { id: "cloud-sky", icon: Cloud, className: "text-sky-500" },
  { id: "cloud-orange", icon: Cloud, className: "text-orange-500" },
  { id: "globe-blue", icon: Globe, className: "text-blue-500" },
  { id: "briefcase-purple", icon: Briefcase, className: "text-purple-500" },
  { id: "user-teal", icon: UserRound, className: "text-teal-500" },
  { id: "star-amber", icon: Star, className: "text-amber-500" },
  { id: "heart-rose", icon: Heart, className: "text-rose-500" },
  { id: "rocket-indigo", icon: Rocket, className: "text-indigo-500" },
  { id: "leaf-green", icon: Leaf, className: "text-green-500" },
  { id: "sun-yellow", icon: Sun, className: "text-yellow-500" },
  { id: "music-pink", icon: Music, className: "text-pink-500" },
  { id: "camera-cyan", icon: Camera, className: "text-cyan-500" },
];

/** Look up an icon definition by id; returns null when unset or unknown. */
export function accountIconDef(id?: string | null): AccountIconDef | null {
  if (!id) return null;
  return ACCOUNT_ICON_LIBRARY.find((def) => def.id === id) ?? null;
}

/** Fallback icon per account kind, used when no custom icon is chosen. */
export function defaultIconForKind(kind: AccountKind): AccountIconDef {
  return kind === "china"
    ? { id: "default-china", icon: Cloud, className: "text-orange-500" }
    : { id: "default-global", icon: Globe, className: "text-blue-500" };
}
