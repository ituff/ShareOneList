import { create } from "zustand";
import type { LlmConfigSnapshot, LlmProviderPreset } from "../lib/types";
import {
  getLlmConfig,
  saveLlmProvider,
  deleteLlmProvider,
  setDefaultModel,
} from "../lib/tauri";

/** Frontend mirror of the Rust preset table; used to pre-fill the form. */
export const PRESET_DEFAULTS: Record<
  LlmProviderPreset,
  { baseUrl: string; kind: "openai_compatible" | "azure_openai"; apiVersion?: string }
> = {
  openai: { baseUrl: "https://api.openai.com/v1", kind: "openai_compatible" },
  "azure-openai": {
    baseUrl: "https://<resource-name>.openai.azure.com",
    kind: "azure_openai",
    apiVersion: "2024-10-21",
  },
  deepseek: { baseUrl: "https://api.deepseek.com/v1", kind: "openai_compatible" },
  dashscope: {
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    kind: "openai_compatible",
  },
  moonshot: { baseUrl: "https://api.moonshot.cn/v1", kind: "openai_compatible" },
  zhipu: { baseUrl: "https://open.bigmodel.cn/api/paas/v4", kind: "openai_compatible" },
  ollama: { baseUrl: "http://localhost:11434/v1", kind: "openai_compatible" },
  custom: { baseUrl: "", kind: "openai_compatible" },
};

export const PRESET_ORDER: LlmProviderPreset[] = [
  "openai",
  "azure-openai",
  "deepseek",
  "dashscope",
  "moonshot",
  "zhipu",
  "ollama",
  "custom",
];

interface LlmState {
  /** Current LLM config snapshot (providers + default model + masked keys). */
  snapshot: LlmConfigSnapshot | null;
  /** Whether the initial config has been loaded from the backend. */
  isLoaded: boolean;
  /** Last error message from a failed operation. */
  error: string | null;

  /** Fetch the LLM config from the backend. */
  loadConfig: () => Promise<void>;
  /** Save a provider (create or update); optionally set/clear its API key. */
  saveProvider: (
    provider: Parameters<typeof saveLlmProvider>[0],
    apiKey?: string | null
  ) => Promise<void>;
  /** Delete a provider and its stored key. */
  deleteProvider: (providerId: string) => Promise<void>;
  /** Change the default model. */
  updateDefaultModel: (providerId: string, modelId: string) => Promise<void>;
  /** Clear the last error. */
  clearError: () => void;
}

export const useLlmStore = create<LlmState>((set) => ({
  snapshot: null,
  isLoaded: false,
  error: null,

  loadConfig: async () => {
    try {
      const snapshot = await getLlmConfig();
      set({ snapshot, isLoaded: true, error: null });
    } catch (e) {
      set({ error: String(e), isLoaded: true });
    }
  },

  saveProvider: async (provider, apiKey) => {
    const snapshot = await saveLlmProvider(provider, apiKey ?? null);
    set({ snapshot, error: null });
  },

  deleteProvider: async (providerId) => {
    const snapshot = await deleteLlmProvider(providerId);
    set({ snapshot, error: null });
  },

  updateDefaultModel: async (providerId, modelId) => {
    const snapshot = await setDefaultModel(providerId, modelId);
    set({ snapshot, error: null });
  },

  clearError: () => set({ error: null }),
}));

/** Convenience selector: all selectable models flattened across providers. */
export function allModels(snapshot: LlmConfigSnapshot | null) {
  if (!snapshot) return [];
  return snapshot.config.providers.flatMap((p) =>
    p.models.map((m) => ({
      providerId: p.id,
      providerName: p.name,
      modelId: m.modelId,
      displayName: m.displayName,
    }))
  );
}

/** Currently selected default model descriptor, if any. */
export function defaultModel(snapshot: LlmConfigSnapshot | null) {
  if (!snapshot?.config.defaultModel) return null;
  const ref = snapshot.config.defaultModel;
  const provider = snapshot.config.providers.find((p) => p.id === ref.providerId);
  const model = provider?.models.find((m) => m.modelId === ref.modelId);
  if (!provider || !model) return null;
  return {
    providerId: provider.id,
    providerName: provider.name,
    modelId: model.modelId,
    displayName: model.displayName,
  };
}

/** True when at least one provider with a stored API key exists. */
export function hasUsableProvider(snapshot: LlmConfigSnapshot | null) {
  return !!snapshot && snapshot.config.providers.some((p) => p.hasApiKey);
}

// Re-export so components don't import from lib/tauri directly.
export { getLlmConfig };
