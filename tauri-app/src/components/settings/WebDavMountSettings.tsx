import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  CheckCircle2,
  Copy,
  FolderInput,
  HardDrive,
  Plus,
  RefreshCw,
  Stethoscope,
  Trash2,
  Wrench,
} from "lucide-react";
import {
  getSharepointSites,
  getSiteDrives,
  webdavApplyFix,
  webdavCopyMountInfo,
  webdavCreateMount,
  webdavDeleteMount,
  webdavDiagnose,
  webdavMount,
  webdavStatus,
  webdavUnmount,
} from "../../lib/tauri";
import type {
  CloudEnvironment,
  Drive,
  Site,
  WebDavDiagnosis,
  WebDavMountStatus,
  WebDavStatus,
} from "../../lib/types";
import { useAuthStore } from "../../stores/authStore";
import { useToastStore } from "../../stores/toastStore";

const isWindows = navigator.userAgent.includes("Windows");

/** The "Cloud drive mounting" tab: manage loopback WebDAV gateway mounts
 * that Explorer (drive letters) / Finder (/Volumes) can attach to. */
export function WebDavMountSettings() {
  const { t } = useTranslation();
  const accounts = useAuthStore((s) => s.accounts);

  const [status, setStatus] = useState<WebDavStatus | null>(null);
  const [diag, setDiag] = useState<WebDavDiagnosis | null>(null);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [revealed, setRevealed] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [s, d] = await Promise.all([webdavStatus(), webdavDiagnose()]);
      setStatus(s);
      setDiag(d);
    } catch (err) {
      console.error("[webdav] status failed:", err);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <>
      <GatewaySection status={status} onRefresh={refresh} />

      <MountListSection
        status={status}
        busy={busy}
        setBusy={setBusy}
        revealed={revealed}
        setRevealed={setRevealed}
        onChanged={refresh}
      />

      {wizardOpen ? (
        <CreateWizard
          accounts={accounts}
          onDone={() => {
            setWizardOpen(false);
            void refresh();
          }}
          onCancel={() => setWizardOpen(false)}
        />
      ) : (
        <section className="space-y-3 rounded-lg border border-border bg-card p-4">
          <button
            onClick={() => setWizardOpen(true)}
            disabled={accounts.length === 0}
            className="inline-flex items-center gap-2 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          >
            <Plus size={15} />
            {t("webdavMount.newMount")}
          </button>
          {accounts.length === 0 && (
            <p className="text-sm text-muted-foreground">{t("webdavMount.needAccount")}</p>
          )}
        </section>
      )}

      {isWindows && diag && <DiagnosisSection diag={diag} onChanged={refresh} />}
    </>
  );
}

function GatewaySection({
  status,
  onRefresh,
}: {
  status: WebDavStatus | null;
  onRefresh: () => Promise<void>;
}) {
  const { t } = useTranslation();
  return (
    <section className="space-y-3 rounded-lg border border-border bg-card p-4">
      <div className="flex items-center justify-between">
        <h3 className="flex items-center gap-2 text-lg font-semibold text-foreground">
          <HardDrive size={18} />
          {t("webdavMount.title")}
        </h3>
        <button
          onClick={() => void onRefresh()}
          className="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground"
          title={t("webdavMount.refresh")}
        >
          <RefreshCw size={15} />
        </button>
      </div>
      <p className="text-sm text-muted-foreground">{t("webdavMount.description")}</p>
      <div className="flex items-center justify-between rounded-md bg-muted/40 px-3 py-2">
        <span className="text-sm text-muted-foreground">{t("webdavMount.gateway")}</span>
        {status?.running ? (
          <span className="text-sm font-medium text-emerald-600 dark:text-emerald-400">
            {t("webdavMount.gatewayRunning", { port: status.port })}
          </span>
        ) : (
          <span className="text-sm font-medium text-muted-foreground">
            {t("webdavMount.gatewayStopped")}
          </span>
        )}
      </div>
    </section>
  );
}

function MountListSection({
  status,
  busy,
  setBusy,
  revealed,
  setRevealed,
  onChanged,
}: {
  status: WebDavStatus | null;
  busy: boolean;
  setBusy: (b: boolean) => void;
  revealed: string | null;
  setRevealed: (id: string | null) => void;
  onChanged: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);

  const run = async (action: () => Promise<unknown>, doneMsg?: string) => {
    setBusy(true);
    try {
      await action();
      if (doneMsg) addToast("success", doneMsg);
      await onChanged();
    } catch (err) {
      addToast("error", mountErrorText(t, String(err)));
    } finally {
      setBusy(false);
    }
  };

  const handleCopyInfo = async (mountId: string) => {
    try {
      const info = await webdavCopyMountInfo(mountId);
      const text = `${info.url}\n${t("webdavMount.copyUser")}: ${info.username}\n${t(
        "webdavMount.copyPass"
      )}: ${info.password}`;
      try {
        await navigator.clipboard.writeText(text);
        addToast("success", t("webdavMount.copied"));
      } catch {
        // clipboard unavailable — reveal instead
        setRevealed(revealed === mountId ? null : mountId);
      }
    } catch (err) {
      addToast("error", String(err));
    }
  };

  const mounts = status?.mounts ?? [];

  return (
    <section className="space-y-3 rounded-lg border border-border bg-card p-4">
      <h3 className="text-lg font-semibold text-foreground">{t("webdavMount.mounts")}</h3>
      {mounts.length === 0 && (
        <p className="text-sm text-muted-foreground">{t("webdavMount.noMounts")}</p>
      )}
      <div className="space-y-2">
        {mounts.map((m: WebDavMountStatus) => (
          <div key={m.mountId} className="rounded-md border border-border bg-background p-3">
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-medium text-foreground">{m.label}</span>
              <EnvBadge env={m.cloudEnv} />
              {m.driveLetter && (
                <span className="rounded bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                  {m.driveLetter}:
                </span>
              )}
              <span
                className={`ml-auto text-xs ${
                  m.mounted
                    ? "font-medium text-emerald-600 dark:text-emerald-400"
                    : "text-muted-foreground"
                }`}
              >
                {m.mounted ? t("webdavMount.mounted") : t("webdavMount.notMounted")}
              </span>
            </div>
            <div className="mt-1 truncate font-mono text-xs text-muted-foreground">{m.url}</div>
            <div className="mt-2 flex flex-wrap gap-2">
              {m.mounted ? (
                <button
                  disabled={busy}
                  onClick={() => void run(() => webdavUnmount(m.mountId), t("webdavMount.unmounted"))}
                  className="rounded-md border border-border px-2.5 py-1 text-xs text-foreground hover:bg-accent disabled:opacity-50"
                >
                  {t("webdavMount.unmount")}
                </button>
              ) : (
                <button
                  disabled={busy || !status?.running}
                  onClick={() => void run(() => webdavMount(m.mountId), t("webdavMount.mountDone"))}
                  className="inline-flex items-center gap-1.5 rounded-md border border-border px-2.5 py-1 text-xs text-foreground hover:bg-accent disabled:opacity-50"
                >
                  <FolderInput size={13} />
                  {t("webdavMount.mount")}
                </button>
              )}
              <button
                disabled={busy}
                onClick={() => void handleCopyInfo(m.mountId)}
                className="inline-flex items-center gap-1.5 rounded-md border border-border px-2.5 py-1 text-xs text-foreground hover:bg-accent disabled:opacity-50"
              >
                <Copy size={13} />
                {t("webdavMount.copyInfo")}
              </button>
              <button
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    await webdavDeleteMount(m.mountId);
                  }, t("webdavMount.deleted"))
                }
                className="inline-flex items-center gap-1.5 rounded-md border border-destructive/40 px-2.5 py-1 text-xs text-destructive hover:bg-destructive/10 disabled:opacity-50"
              >
                <Trash2 size={13} />
                {t("webdavMount.delete")}
              </button>
            </div>
            {revealed === m.mountId && <RevealedInfo mountId={m.mountId} />}
          </div>
        ))}
      </div>
    </section>
  );
}

function RevealedInfo({ mountId }: { mountId: string }) {
  const { t } = useTranslation();
  const [info, setInfo] = useState<{ url: string; username: string; password: string } | null>(
    null
  );
  useEffect(() => {
    void webdavCopyMountInfo(mountId).then(setInfo).catch(() => setInfo(null));
  }, [mountId]);
  if (!info) return null;
  return (
    <div className="mt-2 space-y-1 rounded bg-muted/40 p-2 font-mono text-xs text-muted-foreground">
      <div className="break-all">{info.url}</div>
      <div>
        {t("webdavMount.copyUser")}: {info.username}
      </div>
      <div className="break-all">
        {t("webdavMount.copyPass")}: {info.password}
      </div>
    </div>
  );
}

function EnvBadge({ env }: { env: CloudEnvironment }) {
  const { t } = useTranslation();
  return (
    <span className="rounded bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
      {t(env === "china" ? "webdavMount.envChina" : "webdavMount.envGlobal")}
    </span>
  );
}

function CreateWizard({
  accounts,
  onDone,
  onCancel,
}: {
  accounts: { homeAccountId: string; driveId: string; cloudType: CloudEnvironment; displayName: string; alias?: string | null }[];
  onDone: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const [accountIdx, setAccountIdx] = useState(0);
  const [scope, setScope] = useState<"onedrive" | "site">("onedrive");
  const [sites, setSites] = useState<Site[]>([]);
  const [siteId, setSiteId] = useState("");
  const [drives, setDrives] = useState<Drive[]>([]);
  const [driveId, setDriveId] = useState("");
  const [rootPath, setRootPath] = useState("");
  const [letter, setLetter] = useState("");
  const [creating, setCreating] = useState(false);

  const account = accounts[accountIdx];

  useEffect(() => {
    setSites([]);
    setSiteId("");
    setDrives([]);
    setDriveId("");
    if (scope !== "site" || !account) return;
    void getSharepointSites(account.cloudType, account.homeAccountId)
      .then((list) => {
        setSites(list);
        if (list.length > 0) setSiteId(list[0].id);
      })
      .catch((err) => addToast("error", String(err)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scope, account?.homeAccountId]);

  useEffect(() => {
    setDrives([]);
    setDriveId("");
    if (scope !== "site" || !account || !siteId) return;
    void getSiteDrives(siteId, account.cloudType, account.homeAccountId)
      .then((list) => {
        setDrives(list);
        if (list.length > 0) setDriveId(list[0].id);
      })
      .catch((err) => addToast("error", String(err)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [siteId]);

  const siteName = sites.find((s) => s.id === siteId)?.displayName ?? "";
  const driveName = drives.find((d) => d.id === driveId)?.name ?? "";
  const defaultLabel =
    scope === "onedrive"
      ? account?.alias || account?.displayName || ""
      : [siteName, driveName].filter(Boolean).join(" / ");

  const handleCreate = async () => {
    if (!account) return;
    const chosenDrive = scope === "onedrive" ? account.driveId : driveId;
    if (!chosenDrive) {
      addToast("error", t("webdavMount.errNoDrive"));
      return;
    }
    setCreating(true);
    try {
      await webdavCreateMount({
        cloudEnv: account.cloudType,
        homeAccountId: account.homeAccountId,
        driveId: chosenDrive,
        rootPath: rootPath.trim(),
        label: defaultLabel || t("webdavMount.untitled"),
        driveLetter: isWindows && /^[A-Za-z]$/.test(letter.trim()) ? letter.trim().toUpperCase() : null,
      });
      addToast("success", t("webdavMount.created"));
      onDone();
    } catch (err) {
      addToast("error", mountErrorText(t, String(err)));
    } finally {
      setCreating(false);
    }
  };

  const selectCls =
    "rounded-md border border-border bg-background px-2.5 py-1.5 text-sm text-foreground";

  return (
    <section className="space-y-3 rounded-lg border border-border bg-card p-4">
      <h3 className="text-lg font-semibold text-foreground">{t("webdavMount.newMount")}</h3>

      <div className="grid gap-3 sm:grid-cols-2">
        <label className="space-y-1 text-sm text-muted-foreground">
          {t("webdavMount.account")}
          <select
            value={accountIdx}
            onChange={(e) => setAccountIdx(Number(e.target.value))}
            className={`${selectCls} w-full`}
          >
            {accounts.map((a, i) => (
              <option key={`${a.cloudType}-${a.homeAccountId}`} value={i}>
                {a.alias || a.displayName} (
                {t(a.cloudType === "china" ? "webdavMount.envChina" : "webdavMount.envGlobal")})
              </option>
            ))}
          </select>
        </label>

        <label className="space-y-1 text-sm text-muted-foreground">
          {t("webdavMount.scope")}
          <select
            value={scope}
            onChange={(e) => setScope(e.target.value as "onedrive" | "site")}
            className={`${selectCls} w-full`}
          >
            <option value="onedrive">{t("webdavMount.scopeOneDrive")}</option>
            <option value="site">{t("webdavMount.scopeSite")}</option>
          </select>
        </label>

        {scope === "site" && (
          <>
            <label className="space-y-1 text-sm text-muted-foreground">
              {t("webdavMount.site")}
              <select
                value={siteId}
                onChange={(e) => setSiteId(e.target.value)}
                className={`${selectCls} w-full`}
              >
                {sites.length === 0 && <option value="">{t("webdavMount.loading")}</option>}
                {sites.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.displayName}
                  </option>
                ))}
              </select>
            </label>
            <label className="space-y-1 text-sm text-muted-foreground">
              {t("webdavMount.drive")}
              <select
                value={driveId}
                onChange={(e) => setDriveId(e.target.value)}
                className={`${selectCls} w-full`}
              >
                {drives.length === 0 && <option value="">{t("webdavMount.loading")}</option>}
                {drives.map((d) => (
                  <option key={d.id} value={d.id}>
                    {d.name}
                  </option>
                ))}
              </select>
            </label>
          </>
        )}

        <label className="space-y-1 text-sm text-muted-foreground">
          {t("webdavMount.subfolder")}
          <input
            value={rootPath}
            onChange={(e) => setRootPath(e.target.value)}
            placeholder="/Documents/Projects"
            className={`${selectCls} w-full font-mono`}
          />
        </label>

        {isWindows && (
          <label className="space-y-1 text-sm text-muted-foreground">
            {t("webdavMount.letter")}
            <input
              value={letter}
              onChange={(e) => setLetter(e.target.value)}
              placeholder={t("webdavMount.letterAuto")}
              maxLength={1}
              className={`${selectCls} w-20 uppercase`}
            />
          </label>
        )}
      </div>

      <p className="text-sm text-muted-foreground">
        {t("webdavMount.willCreate")}: <span className="text-foreground">{defaultLabel || "—"}</span>
        {rootPath.trim() && <span className="font-mono">{rootPath.trim()}</span>}
      </p>

      <div className="flex gap-2">
        <button
          onClick={() => void handleCreate()}
          disabled={creating}
          className="rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
        >
          {t("webdavMount.create")}
        </button>
        <button
          onClick={onCancel}
          className="rounded-md border border-border px-3 py-1.5 text-sm text-foreground hover:bg-accent"
        >
          {t("webdavMount.cancel")}
        </button>
      </div>
    </section>
  );
}

function DiagnosisSection({ diag, onChanged }: { diag: WebDavDiagnosis; onChanged: () => Promise<void> }) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const [fixing, setFixing] = useState(false);

  const handleFix = async () => {
    setFixing(true);
    try {
      await webdavApplyFix();
      addToast("success", t("webdavMount.fixLaunched"));
      // give the elevated script a moment, then re-check
      await new Promise((r) => setTimeout(r, 3000));
      await onChanged();
    } catch (err) {
      addToast("error", String(err));
    } finally {
      setFixing(false);
    }
  };

  return (
    <section className="space-y-3 rounded-lg border border-border bg-card p-4">
      <div className="flex items-center justify-between">
        <h3 className="flex items-center gap-2 text-lg font-semibold text-foreground">
          <Stethoscope size={18} />
          {t("webdavMount.diagnosis")}
        </h3>
        {diag.issues.length === 0 ? (
          <span className="inline-flex items-center gap-1.5 text-sm text-emerald-600 dark:text-emerald-400">
            <CheckCircle2 size={15} />
            {t("webdavMount.diagOk")}
          </span>
        ) : (
          <span className="inline-flex items-center gap-1.5 text-sm text-amber-600 dark:text-amber-400">
            <AlertTriangle size={15} />
            {t("webdavMount.diagIssues", { count: diag.issues.length })}
          </span>
        )}
      </div>

      {diag.issues.length > 0 && (
        <ul className="space-y-1">
          {diag.issues.map((code) => (
            <li key={code} className="text-sm text-muted-foreground">
              • {t(`webdavMount.issue.${code}`, { defaultValue: code })}
            </li>
          ))}
        </ul>
      )}

      {diag.fixable && (
        <button
          onClick={() => void handleFix()}
          disabled={fixing}
          className="inline-flex items-center gap-2 rounded-md border border-border px-3 py-1.5 text-sm text-foreground hover:bg-accent disabled:opacity-50"
        >
          <Wrench size={14} />
          {t("webdavMount.applyFix")}
        </button>
      )}
      <p className="text-xs text-muted-foreground">{t("webdavMount.diagHint")}</p>
    </section>
  );
}

/** Map backend error sentinels to localized copy; pass through anything else. */
function mountErrorText(t: (key: string) => string, raw: string): string {
  const sentinels: Record<string, string> = {
    duplicate_mount: "webdavMount.errDuplicate",
    mount_not_found: "webdavMount.errNotFound",
    server_not_running: "webdavMount.errServerDown",
    no_free_drive_letter: "webdavMount.errNoLetter",
    access_denied_check_diagnostics: "webdavMount.errAccessDenied",
    invalid_credentials: "webdavMount.errCredentials",
    webclient_error_check_diagnostics: "webdavMount.errWebclient",
    fix_cancelled_or_failed: "webdavMount.errFixFailed",
    platform_not_supported: "webdavMount.errPlatform",
  };
  for (const [needle, key] of Object.entries(sentinels)) {
    if (raw.includes(needle)) return t(key);
  }
  return raw;
}
