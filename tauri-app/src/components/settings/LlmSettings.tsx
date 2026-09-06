import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { LlmModelConfig, LlmProviderConfig, LlmProviderPreset } from "../../lib/types";
import { testLlmConnection, listLlmModels, formatAppError } from "../../lib/tauri";
import {
  PRESET_DEFAULTS,
  PRESET_ORDER,
  useLlmStore,
} from "../../stores/llmStore";
import { useToastStore } from "../../stores/toastStore";

/** Empty editable provider used when adding a new one. Starts with one
 * empty model row so the "at least one model" rule is obvious. */
function newProvider(preset: LlmProviderPreset): LlmProviderConfig {
  const defaults = PRESET_DEFAULTS[preset];
  return {
    id: "",
    name: "",
    preset,
    kind: defaults.kind,
    baseUrl: defaults.baseUrl,
    apiVersion: defaults.apiVersion ?? null,
    models: [{ id: `model-${Date.now()}`, modelId: "", displayName: "" }],
    hasApiKey: false,
  };
}

/** True when every character of `query` appears in `target` in order. */
function isSubsequence(query: string, target: string): boolean {
  let i = 0;
  for (const ch of target) {
    if (ch === query[i]) i++;
    if (i === query.length) return true;
  }
  return false;
}

/** Case-insensitive fuzzy match: exact < prefix < substring < subsequence,
 * then shorter ids first. Returns at most `limit` ids. */
export function fuzzyFilterModels(suggestions: string[], query: string, limit = 12): string[] {
  const q = query.trim().toLowerCase();
  if (!q) return suggestions.slice(0, limit);
  const scored: { id: string; score: number }[] = [];
  for (const id of suggestions) {
    const lower = id.toLowerCase();
    let score: number;
    if (lower === q) score = 0;
    else if (lower.startsWith(q)) score = 1;
    else if (lower.includes(q)) score = 2;
    else if (isSubsequence(q, lower)) score = 3;
    else continue;
    scored.push({ id, score });
  }
  scored.sort(
    (a, b) => a.score - b.score || a.id.length - b.id.length || a.id.localeCompare(b.id)
  );
  return scored.slice(0, limit).map((s) => s.id);
}

/** Text input with a fuzzy-match dropdown fed by the provider's model list.
 * Free-text entry stays possible for ids not in the fetched list. */
function ModelIdInput({
  value,
  suggestions,
  onSelect,
  onChange,
}: {
  value: string;
  suggestions: string[];
  onSelect: (id: string) => void;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(0);
  const filtered = fuzzyFilterModels(suggestions, value);

  const pick = (id: string) => {
    onSelect(id);
    setOpen(false);
  };

  return (
    <div className="relative min-w-0 flex-1">
      <input
        type="text"
        value={value}
        placeholder={t("llm.modelIdPlaceholder")}
        onChange={(e) => {
          onChange(e.target.value);
          setHighlight(0);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={(e) => {
          if (!open || filtered.length === 0) return;
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setHighlight((h) => (h + 1) % filtered.length);
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setHighlight((h) => (h - 1 + filtered.length) % filtered.length);
          } else if (e.key === "Enter") {
            e.preventDefault();
            pick(filtered[highlight]);
          } else if (e.key === "Escape") {
            setOpen(false);
          }
        }}
        className="w-full rounded-md border border-border bg-background px-2 py-1.5 font-mono text-sm text-foreground"
      />
      {open && filtered.length > 0 && (
        <ul className="absolute z-20 mt-1 max-h-48 w-full overflow-auto rounded-md border border-border bg-background shadow-lg">
          {filtered.map((id, index) => (
            <li key={id}>
              <button
                type="button"
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => pick(id)}
                onMouseEnter={() => setHighlight(index)}
                className={`block w-full truncate px-2 py-1.5 text-left font-mono text-sm ${
                  index === highlight
                    ? "bg-accent text-accent-foreground"
                    : "text-foreground"
                }`}
              >
                {id}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function LlmSettings() {
  const { t } = useTranslation();
  const snapshot = useLlmStore((s) => s.snapshot);
  const isLoaded = useLlmStore((s) => s.isLoaded);
  const loadConfig = useLlmStore((s) => s.loadConfig);
  const saveProvider = useLlmStore((s) => s.saveProvider);
  const deleteProvider = useLlmStore((s) => s.deleteProvider);
  const updateDefaultModel = useLlmStore((s) => s.updateDefaultModel);
  const addToast = useToastStore((s) => s.addToast);

  const [editing, setEditing] = useState<LlmProviderConfig | null>(null);
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [testing, setTesting] = useState(false);
  const [fetchingModels, setFetchingModels] = useState(false);
  /** Model ids reported by the provider's /models endpoint; used as the
   * fuzzy-match dropdown source instead of bulk-adding every model. */
  const [availableModels, setAvailableModels] = useState<string[]>([]);

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  const defaultRef = snapshot?.config.defaultModel ?? null;

  const openAdd = () => {
    setEditing(newProvider("openai"));
    setApiKeyInput("");
    setAvailableModels([]);
  };

  const openEdit = (provider: LlmProviderConfig) => {
    setEditing({ ...provider, models: provider.models.map((m) => ({ ...m })) });
    setApiKeyInput("");
    setAvailableModels([]);
  };

  const handlePresetChange = (preset: LlmProviderPreset) => {
    if (!editing) return;
    const defaults = PRESET_DEFAULTS[preset];
    setEditing({
      ...editing,
      preset,
      kind: defaults.kind,
      baseUrl: defaults.baseUrl || editing.baseUrl,
      apiVersion: defaults.apiVersion ?? null,
    });
  };

  const handleSave = async () => {
    if (!editing) return;
    try {
      await saveProvider(editing, apiKeyInput === "" ? null : apiKeyInput);
      addToast("success", t("llm.toast.saved"));
      setEditing(null);
    } catch (e) {
      addToast("error", formatAppError(e));
    }
  };

  const handleDelete = async (provider: LlmProviderConfig) => {
    if (!window.confirm(t("llm.confirmDelete", { name: provider.name }))) return;
    try {
      await deleteProvider(provider.id);
      addToast("success", t("llm.toast.deleted"));
    } catch (e) {
      addToast("error", formatAppError(e));
    }
  };

  const handleTest = async () => {
    if (!editing) return;
    setTesting(true);
    try {
      await testLlmConnection(editing, apiKeyInput === "" ? null : apiKeyInput);
      addToast("success", t("llm.toast.testOk"));
    } catch (e) {
      addToast("error", formatAppError(e));
    } finally {
      setTesting(false);
    }
  };

  const handleFetchModels = async () => {
    if (!editing) return;
    setFetchingModels(true);
    try {
      const ids = await listLlmModels(editing, apiKeyInput === "" ? null : apiKeyInput);
      setAvailableModels(ids);
      addToast("success", t("llm.toast.modelsFetched", { count: ids.length }));
    } catch (e) {
      addToast("error", formatAppError(e));
    } finally {
      setFetchingModels(false);
    }
  };

  const handleSetDefault = async (providerId: string, modelId: string) => {
    try {
      await updateDefaultModel(providerId, modelId);
    } catch (e) {
      addToast("error", formatAppError(e));
    }
  };

  const updateModel = (index: number, patch: Partial<LlmModelConfig>) => {
    if (!editing) return;
    setEditing({
      ...editing,
      models: editing.models.map((m, i) => (i === index ? { ...m, ...patch } : m)),
    });
  };

  return (
    <section className="space-y-3 rounded-lg border border-border bg-card p-4">
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-semibold text-foreground">{t("llm.title")}</h3>
        <button
          onClick={openAdd}
          className="rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90"
        >
          {t("llm.addProvider")}
        </button>
      </div>
      <p className="text-xs text-muted-foreground">{t("llm.description")}</p>

      {isLoaded && snapshot && snapshot.config.providers.length === 0 && (
        <p className="rounded-md bg-muted/40 px-3 py-4 text-sm text-muted-foreground">
          {t("llm.empty")}
        </p>
      )}

      <div className="space-y-2">
        {snapshot?.config.providers.map((provider) => {
          const isDefaultProvider = defaultRef?.providerId === provider.id;
          return (
            <div
              key={provider.id}
              className="rounded-md border border-border bg-background p-3"
            >
              <div className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-medium text-foreground">{provider.name}</span>
                    <span className="rounded bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                      {t(`llm.presets.${provider.preset}`)}
                    </span>
                    {isDefaultProvider && (
                      <span className="rounded bg-primary/15 px-1.5 py-0.5 text-xs font-medium text-primary">
                        {t("llm.defaultBadge")}
                      </span>
                    )}
                  </div>
                  <p className="mt-1 truncate text-xs text-muted-foreground">{provider.baseUrl}</p>
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {provider.models.map((model) => {
                      const isDefault =
                        isDefaultProvider && defaultRef?.modelId === model.modelId;
                      return (
                        <button
                          key={model.id}
                          onClick={() => !isDefault && handleSetDefault(provider.id, model.modelId)}
                          title={isDefault ? undefined : t("llm.setDefaultHint")}
                          className={`rounded-full px-2.5 py-0.5 text-xs ${
                            isDefault
                              ? "bg-primary text-primary-foreground"
                              : "border border-border text-muted-foreground hover:bg-accent hover:text-foreground"
                          }`}
                        >
                          {model.displayName}
                        </button>
                      );
                    })}
                  </div>
                </div>
                <div className="flex shrink-0 flex-col items-end gap-1">
                  <div className="flex gap-1">
                    <button
                      onClick={() => openEdit(provider)}
                      className="rounded px-2 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-foreground"
                    >
                      {t("llm.edit")}
                    </button>
                    <button
                      onClick={() => handleDelete(provider)}
                      className="rounded px-2 py-1 text-xs text-destructive hover:bg-destructive/10"
                    >
                      {t("llm.delete")}
                    </button>
                  </div>
                  <span className="text-xs text-muted-foreground">
                    {provider.hasApiKey
                      ? t("llm.keyStored")
                      : t("llm.keyMissing")}
                  </span>
                </div>
              </div>
            </div>
          );
        })}
      </div>

      {editing && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
          <div className="max-h-[85vh] w-full max-w-lg overflow-auto rounded-lg border border-border bg-background p-4 shadow-xl">
            <h4 className="mb-3 text-base font-semibold text-foreground">
              {editing.id ? t("llm.editProvider") : t("llm.addProvider")}
            </h4>

            <div className="space-y-3">
              <label className="block space-y-1">
                <span className="text-sm text-muted-foreground">{t("llm.preset")}</span>
                <select
                  value={editing.preset}
                  onChange={(e) => handlePresetChange(e.target.value as LlmProviderPreset)}
                  className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground"
                >
                  {PRESET_ORDER.map((preset) => (
                    <option key={preset} value={preset}>
                      {t(`llm.presets.${preset}`)}
                    </option>
                  ))}
                </select>
              </label>

              <label className="block space-y-1">
                <span className="text-sm text-muted-foreground">{t("llm.providerName")}</span>
                <input
                  type="text"
                  value={editing.name}
                  onChange={(e) => setEditing({ ...editing, name: e.target.value })}
                  className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground"
                />
              </label>

              <label className="block space-y-1">
                <span className="text-sm text-muted-foreground">{t("llm.baseUrl")}</span>
                <input
                  type="text"
                  value={editing.baseUrl}
                  onChange={(e) => setEditing({ ...editing, baseUrl: e.target.value })}
                  className="w-full rounded-md border border-border bg-background px-3 py-2 font-mono text-sm text-foreground"
                />
              </label>

              {editing.kind === "azure_openai" && (
                <label className="block space-y-1">
                  <span className="text-sm text-muted-foreground">{t("llm.apiVersion")}</span>
                  <input
                    type="text"
                    value={editing.apiVersion ?? ""}
                    onChange={(e) =>
                      setEditing({ ...editing, apiVersion: e.target.value || null })
                    }
                    className="w-full rounded-md border border-border bg-background px-3 py-2 font-mono text-sm text-foreground"
                  />
                </label>
              )}

              <label className="block space-y-1">
                <span className="text-sm text-muted-foreground">{t("llm.apiKey")}</span>
                <input
                  type="password"
                  value={apiKeyInput}
                  onChange={(e) => setApiKeyInput(e.target.value)}
                  placeholder={
                    editing.hasApiKey
                      ? t("llm.apiKeyPlaceholderStored")
                      : t("llm.apiKeyPlaceholder")
                  }
                  className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground"
                />
                <span className="text-xs text-muted-foreground">{t("llm.apiKeyHint")}</span>
              </label>

              <div className="space-y-1">
                <div className="flex items-center justify-between">
                  <span className="text-sm text-muted-foreground">{t("llm.models")}</span>
                  {editing.kind !== "azure_openai" && (
                    <button
                      onClick={handleFetchModels}
                      disabled={fetchingModels}
                      className="rounded px-2 py-0.5 text-xs text-primary hover:bg-accent disabled:opacity-50"
                    >
                      {fetchingModels ? t("llm.fetchingModels") : t("llm.fetchModels")}
                    </button>
                  )}
                </div>
                {availableModels.length > 0 && (
                  <p className="text-xs text-muted-foreground">
                    {t("llm.modelsAvailableHint", { count: availableModels.length })}
                  </p>
                )}
                {editing.models.map((model, index) => (
                  <div key={index} className="flex gap-1.5">
                    <ModelIdInput
                      value={model.modelId}
                      suggestions={availableModels}
                      onChange={(value) => updateModel(index, { modelId: value })}
                      onSelect={(id) =>
                        updateModel(index, {
                          modelId: id,
                          displayName: model.displayName || id,
                        })
                      }
                    />
                    <input
                      type="text"
                      value={model.displayName}
                      placeholder={t("llm.modelNamePlaceholder")}
                      onChange={(e) => updateModel(index, { displayName: e.target.value })}
                      className="min-w-0 flex-1 rounded-md border border-border bg-background px-2 py-1.5 text-sm text-foreground"
                    />
                    <button
                      onClick={() =>
                        setEditing({
                          ...editing,
                          models: editing.models.filter((_, i) => i !== index),
                        })
                      }
                      className="shrink-0 rounded px-2 text-sm text-destructive hover:bg-destructive/10"
                    >
                      ×
                    </button>
                  </div>
                ))}
                <button
                  onClick={() =>
                    setEditing({
                      ...editing,
                      models: [
                        ...editing.models,
                        {
                          id: `model-${Date.now()}`,
                          modelId: "",
                          displayName: "",
                        },
                      ],
                    })
                  }
                  className="rounded border border-dashed border-border px-2 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-foreground"
                >
                  + {t("llm.addModel")}
                </button>
              </div>
            </div>

            <div className="mt-4 flex justify-between gap-2">
              <button
                onClick={handleTest}
                disabled={testing}
                className="rounded-md border border-border px-3 py-1.5 text-sm text-foreground hover:bg-accent disabled:opacity-50"
              >
                {testing ? t("llm.testing") : t("llm.testConnection")}
              </button>
              <div className="flex gap-2">
                <button
                  onClick={() => setEditing(null)}
                  className="rounded-md px-3 py-1.5 text-sm text-muted-foreground hover:bg-accent hover:text-foreground"
                >
                  {t("llm.cancel")}
                </button>
                <button
                  onClick={handleSave}
                  className="rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90"
                >
                  {t("llm.save")}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
