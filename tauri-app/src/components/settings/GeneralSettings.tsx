import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { enable, isEnabled, disable } from "@tauri-apps/plugin-autostart";
import { useSettingsStore } from "../../stores/settingsStore";
import { useToastStore } from "../../stores/toastStore";
import { formatAppError } from "../../lib/tauri";

/** One labeled checkbox row. */
function ToggleRow({
  checked,
  disabled,
  label,
  hint,
  onChange,
}: {
  checked: boolean;
  disabled?: boolean;
  label: string;
  hint?: string;
  onChange: (next: boolean) => void;
}) {
  return (
    <div>
      <label className="flex cursor-pointer items-center gap-2 text-sm text-foreground">
        <input
          type="checkbox"
          checked={checked}
          disabled={disabled}
          onChange={(e) => onChange(e.target.checked)}
        />
        {label}
      </label>
      {hint && <p className="ml-6 text-xs text-muted-foreground">{hint}</p>}
    </div>
  );
}

/**
 * General app-behavior settings: close-to-tray and launch-at-login. Lives on
 * the appearance tab. The autostart toggle talks to the OS registration
 * directly (tauri-plugin-autostart); the OS is the source of truth, so the
 * checkbox state is read from the plugin on mount.
 */
export function GeneralSettings() {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const closeToTray = useSettingsStore((s) => s.closeToTray);
  const setCloseToTray = useSettingsStore((s) => s.setCloseToTray);
  const [autostart, setAutostart] = useState(false);
  const [autostartReady, setAutostartReady] = useState(false);

  useEffect(() => {
    let cancelled = false;
    isEnabled()
      .then((enabled) => {
        if (!cancelled) {
          setAutostart(enabled);
          setAutostartReady(true);
        }
      })
      .catch((err: unknown) => {
        // Registration failure (e.g. locked-down policy) must not block the
        // other settings — show the toggle unchecked and report on use.
        console.warn("[general-settings] autostart probe failed:", err);
        if (!cancelled) setAutostartReady(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleAutostart = (next: boolean) => {
    const apply = next ? enable() : disable();
    apply
      .then(() => setAutostart(next))
      .catch((err: unknown) => addToast("error", formatAppError(err)));
  };

  return (
    <section className="space-y-3 rounded-lg border border-border bg-card p-4">
      <h3 className="text-lg font-semibold text-foreground">{t("settings.general")}</h3>
      <ToggleRow
        checked={closeToTray}
        label={t("settings.closeToTray")}
        hint={t("settings.closeToTrayHint")}
        onChange={setCloseToTray}
      />
      <ToggleRow
        checked={autostart}
        disabled={!autostartReady}
        label={t("settings.autostart")}
        hint={t("settings.autostartHint")}
        onChange={handleAutostart}
      />
    </section>
  );
}
