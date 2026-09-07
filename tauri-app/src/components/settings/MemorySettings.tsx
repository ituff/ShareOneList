import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { MemoryEntry } from "../../lib/types";
import { formatAppError } from "../../lib/tauri";
import {
  getLlmConfig,
  memoryDelete,
  memoryExtractNow,
  memoryList,
  memorySave,
  memorySearchAndDelete,
  memorySetConfig,
  memorySetEnabled,
  memorySetPinned,
} from "../../lib/tauri";
import { useToastStore } from "../../stores/toastStore";

/** Settings section for the AI memory feature: toggle, extraction
 * threshold, and full list management. */
export function MemorySettings() {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);

  const [entries, setEntries] = useState<MemoryEntry[]>([]);
  const [enabled, setEnabled] = useState(true);
  const [extractEvery, setExtractEvery] = useState(6);
  const [newContent, setNewContent] = useState("");
  const [editingId, setEditingId] = useState("");
  const [editingContent, setEditingContent] = useState("");
  const [forgetKeyword, setForgetKeyword] = useState("");
  const [conversationId, setConversationId] = useState("");
  const [extracting, setExtracting] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [list, config] = await Promise.all([memoryList(), getLlmConfig()]);
      setEntries(list);
      setEnabled(config.config.memory.enabled);
      setExtractEvery(config.config.memory.extractEvery);
    } catch (e) {
      addToast("error", formatAppError(e));
    }
  }, [addToast]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const guard = async (action: () => Promise<unknown>) => {
    try {
      await action();
      await refresh();
    } catch (e) {
      addToast("error", formatAppError(e));
    }
  };

  return (
    <section className="space-y-3 rounded-lg border border-border bg-card p-4">
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-semibold text-foreground">{t("memory.title")}</h3>
        <label className="flex cursor-pointer items-center gap-2 text-sm text-muted-foreground">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => {
              const next = e.target.checked;
              setEnabled(next);
              memorySetConfig(next, extractEvery).catch((err: unknown) =>
                addToast("error", formatAppError(err))
              );
            }}
          />
          {t("memory.enabled")}
        </label>
      </div>
      <p className="text-xs text-muted-foreground">
        {t("memory.privacyNotice")}
      </p>

      <div className="flex items-center gap-2 text-sm">
        <span className="text-muted-foreground">{t("memory.extractEvery")}</span>
        <select
          value={extractEvery}
          disabled={!enabled}
          onChange={(e) => {
            const next = Number(e.target.value);
            setExtractEvery(next);
            memorySetConfig(enabled, next).catch((err: unknown) =>
              addToast("error", formatAppError(err))
            );
          }}
          className="rounded-md border border-border bg-background px-2 py-1 text-sm text-foreground"
        >
          {[3, 6, 12].map((n) => (
            <option key={n} value={n}>
              {t("memory.extractEveryOption", { count: n })}
            </option>
          ))}
        </select>
      </div>

      <div className="space-y-2">
        {entries.length === 0 && (
          <p className="rounded-md bg-muted/40 px-3 py-4 text-sm text-muted-foreground">
            {t("memory.empty")}
          </p>
        )}
        {entries.map((entry) => (
          <div key={entry.id} className="rounded-md border border-border bg-background p-3">
            {editingId === entry.id ? (
              <div className="space-y-2">
                <textarea
                  value={editingContent}
                  onChange={(e) => setEditingContent(e.target.value)}
                  rows={2}
                  className="w-full rounded-md border border-border bg-background px-2 py-1.5 text-sm text-foreground"
                />
                <div className="flex gap-2">
                  <button
                    onClick={() => {
                      memorySave(entry.id, editingContent)
                        .then(refresh)
                        .then(() => setEditingId(""))
                        .catch((e) => addToast("error", formatAppError(e)));
                    }}
                    className="rounded bg-primary px-2 py-1 text-xs text-primary-foreground hover:bg-primary/90"
                  >
                    {t("memory.saveEdit")}
                  </button>
                  <button
                    onClick={() => setEditingId("")}
                    className="rounded px-2 py-1 text-xs text-muted-foreground hover:bg-accent"
                  >
                    {t("memory.cancel")}
                  </button>
                </div>
              </div>
            ) : (
              <>
                <p className="break-words text-sm text-foreground">{entry.content}</p>
                <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                  {entry.pinned && (
                    <span className="rounded bg-primary/15 px-1.5 py-0.5 text-primary">
                      {t("memory.pinned")}
                    </span>
                  )}
                  {!entry.enabled && <span>{t("memory.disabled")}</span>}
                  <span>
                    {t("memory.useCount", { count: entry.useCount })} ·{" "}
                    {new Date(entry.updatedAt * 1000).toLocaleDateString()}
                  </span>
                  <button
                    onClick={() => guard(() => memorySetEnabled(entry.id, !entry.enabled))}
                    className="rounded px-1.5 py-0.5 hover:bg-accent hover:text-foreground"
                  >
                    {entry.enabled ? t("memory.disable") : t("memory.enable")}
                  </button>
                  <button
                    onClick={() => guard(() => memorySetPinned(entry.id, !entry.pinned))}
                    className="rounded px-1.5 py-0.5 hover:bg-accent hover:text-foreground"
                  >
                    {entry.pinned ? t("memory.unpin") : t("memory.pin")}
                  </button>
                  <button
                    onClick={() => {
                      setEditingId(entry.id);
                      setEditingContent(entry.content);
                    }}
                    className="rounded px-1.5 py-0.5 hover:bg-accent hover:text-foreground"
                  >
                    {t("memory.edit")}
                  </button>
                  <button
                    onClick={() => guard(() => memoryDelete(entry.id))}
                    className="rounded px-1.5 py-0.5 text-destructive hover:bg-destructive/10"
                  >
                    {t("memory.delete")}
                  </button>
                </div>
              </>
            )}
          </div>
        ))}
      </div>

      <div className="space-y-2 border-t border-border pt-3">
        <div className="flex gap-1.5">
          <input
            type="text"
            value={newContent}
            onChange={(e) => setNewContent(e.target.value)}
            placeholder={t("memory.newPlaceholder")}
            className="flex-1 rounded-md border border-border bg-background px-2 py-1.5 text-sm text-foreground"
          />
          <button
            onClick={() => {
              if (!newContent.trim()) return;
              guard(async () => {
                await memorySave("", newContent.trim());
                setNewContent("");
              });
            }}
            disabled={!newContent.trim()}
            className="rounded-md bg-primary px-3 py-1.5 text-sm text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          >
            {t("memory.add")}
          </button>
        </div>

        <div className="flex gap-1.5">
          <input
            type="text"
            value={forgetKeyword}
            onChange={(e) => setForgetKeyword(e.target.value)}
            placeholder={t("memory.forgetPlaceholder")}
            className="flex-1 rounded-md border border-border bg-background px-2 py-1.5 text-sm text-foreground"
          />
          <button
            onClick={() => {
              if (!forgetKeyword.trim()) return;
              guard(async () => {
                const removed = await memorySearchAndDelete(forgetKeyword.trim());
                addToast(
                  removed > 0 ? "success" : "info",
                  t("memory.forgotCount", { count: removed })
                );
                setForgetKeyword("");
              });
            }}
            disabled={!forgetKeyword.trim()}
            className="rounded-md border border-border px-3 py-1.5 text-sm text-foreground hover:bg-accent disabled:opacity-50"
          >
            {t("memory.forget")}
          </button>
        </div>

        <div className="flex items-center gap-2">
          <input
            type="text"
            value={conversationId}
            onChange={(e) => setConversationId(e.target.value)}
            placeholder={t("memory.extractConversationPlaceholder")}
            className="flex-1 rounded-md border border-border bg-background px-2 py-1.5 text-xs text-foreground"
          />
          <button
            onClick={() => {
              setExtracting(true);
              memoryExtractNow(conversationId.trim())
                .then((count) => {
                  addToast("success", t("memory.extracted", { count }));
                  return refresh();
                })
                .catch((e) => addToast("error", formatAppError(e)))
                .finally(() => setExtracting(false));
            }}
            disabled={extracting || !enabled || !conversationId.trim()}
            className="rounded-md border border-border px-3 py-1.5 text-xs text-foreground hover:bg-accent disabled:opacity-50"
          >
            {extracting ? t("memory.extracting") : t("memory.extractNow")}
          </button>
        </div>
      </div>
    </section>
  );
}
