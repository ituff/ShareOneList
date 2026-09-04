import {
  accountIconDef,
  defaultIconForKind,
  resolveAccountKind,
} from "../../lib/account";
import type { AccountEntry } from "../../lib/types";

interface AccountIconProps {
  account: Pick<AccountEntry, "cloudType" | "accountType" | "icon">;
  /** Tailwind size classes; defaults to h-4 w-4. */
  className?: string;
}

/**
 * The account's avatar icon: the user-chosen library icon when set,
 * otherwise a cloud/globe glyph based on the account kind.
 */
export function AccountIcon({ account, className }: AccountIconProps) {
  const def =
    accountIconDef(account.icon) ??
    defaultIconForKind(
      resolveAccountKind(account.cloudType, account.accountType ?? null)
    );
  const Icon = def.icon;
  return (
    <Icon className={`${className ?? "h-4 w-4"} ${def.className} shrink-0`} />
  );
}
