import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { PanelLeftClose, PanelLeftOpen, Search } from "lucide-react";
import type {
  ChatConversationMeta,
  CloudEnvironment,
  DriveItem,
  LlmChatEvent,
  LlmChatMessage,
  LlmContextFile,
  StoredChatMessage,
  StoredContextFile,
} from "../../lib/types";
import {
  cancelLlmChat,
  chatAppendMessage,
  chatDeleteConversation,
  chatListConversations,
  chatNewConversation,
  chatOpenConversation,
  extractFileText,
  formatAppError,
  getTextContent,
  llmChat,
  newLlmRequestId,
  searchFiles,
} from "../../lib/tauri";
import { useAuthStore } from "../../stores/authStore";
import { useNavigationStore } from "../../stores/navigationStore";
import { useTabStore } from "../../stores/tabStore";
import { Markdown } from "./Markdown";
import {
  allModels,
  defaultModel,
  hasUsableProvider,
  useLlmStore,
} from "../../stores/llmStore";

const LLM_CHAT_EVENT = "llm-chat-event";

/** localStorage key for the last selected M365 accounts (null = all). */
const ACCOUNT_SELECTION_KEY = "chat.selectedAccountKeys";
/** localStorage key for the reasoning effort choice. */
const REASONING_EFFORT_KEY = "chat.reasoningEffort";

export type ReasoningEffort = "low" | "medium" | "high";

function loadStoredAccountKeys(): string[] | null {
  try {
    const raw = localStorage.getItem(ACCOUNT_SELECTION_KEY);
    return raw === null ? null : (JSON.parse(raw) as string[]);
  } catch {
    return null;
  }
}

function loadReasoningEffort(): ReasoningEffort {
  const raw = localStorage.getItem(REASONING_EFFORT_KEY);
  return raw === "low" || raw === "medium" ? raw : "high";
}

const CHAT_LIST_COLLAPSED_KEY = "chat-list-collapsed";

function readChatListCollapsed(): boolean {
  try {
    return localStorage.getItem(CHAT_LIST_COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

/** Floating panel anchored above the input bar; closes on outside click. */
function Popover({
  label,
  direction = "up",
  children,
}: {
  label: ReactNode;
  /** "up" opens above the anchor (input bar); "down" below (filter bar). */
  direction?: "up" | "down";
  children: (close: () => void) => ReactNode;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div className="relative">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-foreground"
      >
        {label}
        <span className="text-[10px]">▾</span>
      </button>
      {open && (
        <>
          <div className="fixed inset-0 z-30" onMouseDown={() => setOpen(false)} />
          <div className={`absolute left-0 z-40 max-h-72 min-w-56 overflow-auto rounded-lg border border-border bg-background shadow-lg ${direction === "up" ? "bottom-full mb-2" : "top-full mt-2"}`}>
            {children(() => setOpen(false))}
          </div>
        </>
      )}
    </div>
  );
}

/** M365 account multi-select. `selectedKeys` holds the stored selection
 * (null = all accounts); effective membership is the intersection with the
 * accounts that actually exist right now. */
function AccountPicker({
  accounts,
  selectedKeys,
  onChange,
}: {
  accounts: { homeAccountId: string; cloudType: CloudEnvironment; label: string }[];
  selectedKeys: string[] | null;
  onChange: (next: string[] | null) => void;
}) {
  const { t } = useTranslation();
  const allKeys = accounts.map((a) => `${a.homeAccountId}:${a.cloudType}`);
  const effective = new Set(
    selectedKeys === null ? allKeys : allKeys.filter((k) => selectedKeys.includes(k))
  );

  const toggle = (key: string) => {
    const base = selectedKeys === null ? allKeys : allKeys.filter((k) => selectedKeys.includes(k));
    const next = base.includes(key) ? base.filter((k) => k !== key) : [...base, key];
    // Selecting every account collapses back to the "all" representation.
    onChange(next.length === allKeys.length ? null : next);
  };

  let labelText: string;
  if (effective.size === allKeys.length) {
    labelText = t("home.allAccounts");
  } else if (effective.size === 1) {
    labelText = accounts.find((a) => effective.has(`${a.homeAccountId}:${a.cloudType}`))?.label ?? "";
  } else {
    labelText = t("home.accountsCount", { count: effective.size });
  }

  return (
    <Popover
      label={
        <>
          <span aria-hidden>🗂</span>
          <span className="max-w-40 truncate">{labelText}</span>
        </>
      }
    >
      {(close) => (
        <ul className="py-1">
          <li>
            <button
              type="button"
              onClick={() => {
                onChange(null);
                close();
              }}
              className="block w-full px-3 py-1.5 text-left text-sm text-foreground hover:bg-accent"
            >
              {t("home.selectAllAccounts")}
            </button>
          </li>
          {accounts.map((account) => {
            const key = `${account.homeAccountId}:${account.cloudType}`;
            const checked = effective.has(key);
            return (
              <li key={key}>
                <label className="flex cursor-pointer items-center gap-2 px-3 py-1.5 text-sm text-foreground hover:bg-accent">
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={() => toggle(key)}
                    className="accent-[var(--primary)]"
                  />
                  <span className="truncate">{account.label}</span>
                </label>
              </li>
            );
          })}
        </ul>
      )}
    </Popover>
  );
}

/** Model picker shown in the input bar. */
function ModelPicker({
  models,
  value,
  onChange,
  defaultRef,
}: {
  models: { providerId: string; providerName: string; modelId: string; displayName: string }[];
  value: string;
  onChange: (value: string) => void;
  defaultRef: { providerId: string; modelId: string } | null;
}) {
  const { t } = useTranslation();
  const current = models.find((m) => `${m.providerId}:${m.modelId}` === value);
  return (
    <Popover label={<span className="max-w-40 truncate font-medium text-foreground">{current?.displayName ?? t("home.selectModel")}</span>}>
      {(close) => (
        <ul className="py-1">
          {models.map((m) => {
            const key = `${m.providerId}:${m.modelId}`;
            const isDefault = defaultRef?.providerId === m.providerId && defaultRef?.modelId === m.modelId;
            return (
              <li key={key}>
                <button
                  type="button"
                  onClick={() => {
                    onChange(key);
                    close();
                  }}
                  className={`block w-full px-3 py-1.5 text-left text-sm hover:bg-accent ${
                    key === value ? "text-primary" : "text-foreground"
                  }`}
                >
                  <span className="block truncate">
                    {m.displayName}
                    {isDefault ? t("home.defaultModelSuffix") : ""}
                  </span>
                  <span className="block truncate text-xs text-muted-foreground">{m.providerName}</span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </Popover>
  );
}

/** Reasoning effort picker for reasoning models. */
function EffortPicker({
  value,
  onChange,
}: {
  value: ReasoningEffort;
  onChange: (value: ReasoningEffort) => void;
}) {
  const { t } = useTranslation();
  const options: { value: ReasoningEffort; label: string }[] = [
    { value: "low", label: t("home.effortLow") },
    { value: "medium", label: t("home.effortMedium") },
    { value: "high", label: t("home.effortHigh") },
  ];
  return (
    <Popover label={<><span aria-hidden>⏱</span>{options.find((o) => o.value === value)?.label}</>}>
      {(close) => (
        <ul className="py-1">
          {options.map((option) => (
            <li key={option.value}>
              <button
                type="button"
                onClick={() => {
                  onChange(option.value);
                  close();
                }}
                className={`block w-full px-3 py-1.5 text-left text-sm hover:bg-accent ${
                  option.value === value ? "text-primary" : "text-foreground"
                }`}
              >
                {option.label}
              </button>
            </li>
          ))}
        </ul>
      )}
    </Popover>
  );
}

/** How many cloud file hits are injected into the system prompt per question. */
const CONTEXT_FILE_LIMIT = 8;
/** How many small text files get their content read into the prompt. */
const CONTEXT_READ_LIMIT = 3;
/** Max characters of one file excerpt. */
const EXCERPT_CHAR_LIMIT = 4000;
/** Files above this size are name-only context. */
const READABLE_SIZE_LIMIT = 200_000;
/** Office/PDF files up to this size are parsed into text by the backend. */
const EXTRACTABLE_SIZE_LIMIT = 5_000_000;

const TEXT_EXTENSIONS = new Set([
  "txt", "md", "markdown", "csv", "json", "xml", "log", "yml", "yaml",
  "html", "htm", "ini", "cfg", "conf", "properties", "sql",
  "py", "js", "ts", "tsx", "jsx", "java", "c", "cpp", "h", "cs", "go", "rs",
  "sh", "bat", "ps1",
]);

/** Formats the backend can parse into plain text (see content/extract.rs). */
const EXTRACTABLE_EXTENSIONS = new Set(["docx", "pptx", "xlsx", "pdf"]);

function fileExtension(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot < 0 ? "" : name.slice(dot + 1).toLowerCase();
}

/** How (and whether) a file's content can be included in the AI context. */
function contextReadKind(item: DriveItem): "text" | "extractable" | null {
  if (item.isFolder) return null;
  const ext = fileExtension(item.name);
  if (EXTRACTABLE_EXTENSIONS.has(ext)) {
    return item.size != null && item.size > EXTRACTABLE_SIZE_LIMIT ? null : "extractable";
  }
  if (item.size != null && item.size > READABLE_SIZE_LIMIT) return null;
  // Extensionless files are usually text.
  return ext === "" || TEXT_EXTENSIONS.has(ext) ? "text" : null;
}

/** A cloud file kept client-side for the chat UI: what the model sees
 * (via toLlmContextFile) plus what is needed to open the file in a preview
 * tab (citation chips). Same shape as the persisted StoredContextFile, so
 * history round-trips without mapping. */
type ContextFile = StoredContextFile;

function toLlmContextFile(file: ContextFile): LlmContextFile {
  return {
    name: file.item.name,
    path: file.path,
    webUrl: file.item.webUrl ?? "",
    accountName: file.accountName,
    excerpt: file.excerpt ?? null,
  };
}

/** Searches the given accounts' drives for the question so the model can
 * ground its answer in the user's cloud files. Small text files get their
 * content read (truncated) into the prompt. Search/read failures are
 * ignored — an empty context is valid (the model will then ask before
 * falling back to public knowledge). */
async function gatherCloudContext(
  query: string,
  accounts: { homeAccountId: string; driveId: string; cloudType: CloudEnvironment; displayName: string; alias?: string | null }[]
): Promise<ContextFile[]> {
  const results = await Promise.all(
    accounts.map(async (account) => {
      try {
        const items = await searchFiles(account.driveId, query, "global", account.cloudType);
        return items.map((item) => ({
          item,
          driveId: account.driveId,
          cloudEnv: account.cloudType,
          homeAccountId: account.homeAccountId,
          accountName: account.alias || account.displayName,
          path: item.parentReference?.path ?? "",
        }));
      } catch {
        return [];
      }
    })
  );
  const files: ContextFile[] = results.flat().slice(0, CONTEXT_FILE_LIMIT);

  // Read content for the first few small text/Office/PDF files, in
  // search-relevance order.
  let reads = 0;
  for (let i = 0; i < files.length && reads < CONTEXT_READ_LIMIT; i++) {
    const kind = contextReadKind(files[i].item);
    if (!kind) continue;
    reads++;
    try {
      const content =
        kind === "extractable"
          ? await extractFileText(
              files[i].driveId,
              files[i].item.id,
              files[i].item.name,
              files[i].cloudEnv,
              files[i].homeAccountId
            )
          : await getTextContent(
              files[i].driveId,
              files[i].item.id,
              files[i].cloudEnv,
              files[i].homeAccountId
            );
      files[i] = { ...files[i], excerpt: content.slice(0, EXCERPT_CHAR_LIMIT) };
    } catch {
      // Unreadable (permissions, parse failure, …) — keep name-only.
    }
  }
  return files;
}

interface ChatEntry {
  role: "user" | "assistant";
  content: string;
  /** Cloud files that were provided as context for this message; rendered
   * as clickable citation chips. */
  contextFiles?: ContextFile[];
  /** Chain-of-thought streamed by reasoning models, shown muted. */
  reasoning?: string;
  /** Wall-clock start of the reasoning phase, for the "思考 · 持续了几秒" label. */
  reasoningStartedAt?: number;
  reasoningMs?: number;
  /** Populated while the assistant reply is still streaming. */
  isStreaming?: boolean;
  /** Error shown in place of / below the message. */
  error?: string | null;
}

/** Splits a reasoning duration for the "思考 · 持续了几秒" label. */
function splitReasoningDuration(ms: number): { m: number; s: number } {
  const totalSeconds = Math.max(1, Math.round(ms / 1000));
  return { m: Math.floor(totalSeconds / 60), s: totalSeconds % 60 };
}

/** ZCode-style chain-of-thought line: collapsed to "思考 · 持续了几秒" at
 * the answer position once done; expanded live while thinking. A plain
 * <details> would reset the user's toggle on every streaming re-render,
 * so openness is controlled state that auto-collapses on completion. */
function ReasoningBlock({
  text,
  thinking,
  durationMs,
}: {
  text: string;
  thinking: boolean;
  durationMs?: number;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(true);
  useEffect(() => {
    if (!thinking) setOpen(false);
  }, [thinking]);
  const split = durationMs && durationMs > 0 ? splitReasoningDuration(durationMs) : null;
  const duration = split
    ? split.m > 0
      ? t("home.durationMinutes", { m: split.m, s: split.s })
      : t("home.durationSeconds", { n: split.s })
    : "";
  return (
    <div className="mb-1">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
      >
        <span className={`inline-block text-[10px] transition-transform ${open ? "rotate-180" : ""}`}>
          ⌄
        </span>
        <span>{t("home.reasoningTitle")}</span>
        {duration && <span>· {duration}</span>}
        {thinking && <span className="animate-pulse">…</span>}
      </button>
      {open && (
        <p className="mt-1 whitespace-pre-wrap break-words border-l-2 border-border pl-2 text-xs leading-relaxed text-muted-foreground">
          {text}
          {thinking && <span className="animate-pulse">▍</span>}
        </p>
      )}
    </div>
  );
}

interface SearchResultGroup {
  accountName: string;
  items: DriveItem[];
  driveId: string;
  cloudEnv: CloudEnvironment;
  homeAccountId: string;
}

// ─── Search result filters ──────────────────────────────────────────────────

type SearchTypeFilter = "all" | "folder" | "doc" | "sheet" | "slide" | "image" | "other";
type SearchDateFilter = "all" | "today" | "week" | "month" | "year";

const DOC_EXTS = new Set(["pdf", "doc", "docx", "txt", "md", "rtf", "odt", "log"]);
const SHEET_EXTS = new Set(["xls", "xlsx", "csv", "ods"]);
const SLIDE_EXTS = new Set(["ppt", "pptx", "odp"]);
const IMAGE_EXTS = new Set(["png", "jpg", "jpeg", "gif", "bmp", "webp", "svg", "heic", "tif", "tiff"]);

function fileKind(item: DriveItem): SearchTypeFilter {
  if (item.isFolder) return "folder";
  const dot = item.name.lastIndexOf(".");
  const ext = dot < 0 ? "" : item.name.slice(dot + 1).toLowerCase();
  if (DOC_EXTS.has(ext)) return "doc";
  if (SHEET_EXTS.has(ext)) return "sheet";
  if (SLIDE_EXTS.has(ext)) return "slide";
  if (IMAGE_EXTS.has(ext)) return "image";
  return "other";
}

function withinDateRange(lastModified: string | null, range: SearchDateFilter): boolean {
  if (range === "all") return true;
  if (!lastModified) return false;
  const time = new Date(lastModified).getTime();
  if (Number.isNaN(time)) return false;
  const now = Date.now();
  const startOfToday = new Date();
  startOfToday.setHours(0, 0, 0, 0);
  switch (range) {
    case "today":
      return time >= startOfToday.getTime();
    case "week":
      return time >= now - 7 * 86_400_000;
    case "month":
      return time >= now - 30 * 86_400_000;
    case "year":
      return time >= now - 365 * 86_400_000;
  }
}

/** Single-select filter dropdown used by the search filter bar. */
function FilterPicker({
  icon,
  fallbackLabel,
  options,
  value,
  onChange,
}: {
  icon: string;
  fallbackLabel: string;
  options: { value: string; label: string }[];
  value: string;
  onChange: (value: string) => void;
}) {
  const current = options.find((o) => o.value === value);
  return (
    <Popover
      direction="down"
      label={
        <>
          <span aria-hidden>{icon}</span>
          <span className="max-w-32 truncate">{current?.label ?? fallbackLabel}</span>
        </>
      }
    >
      {(close) => (
        <ul className="py-1">
          {options.map((option) => (
            <li key={option.value}>
              <button
                type="button"
                onClick={() => {
                  onChange(option.value);
                  close();
                }}
                className={`block w-full px-3 py-1.5 text-left text-sm hover:bg-accent ${
                  option.value === value ? "text-primary" : "text-foreground"
                }`}
              >
                {option.label}
              </button>
            </li>
          ))}
        </ul>
      )}
    </Popover>
  );
}

export function HomePage({ initialMode }: { initialMode?: "chat" | "search" }) {
  const [mode, setMode] = useState<"entry" | "chat" | "search">(initialMode ?? "entry");
  const openWithQuery = useNavigationStore((s) => s.openWithQuery);

  // Navigating to the askai/search tab: switch the section (carrying the
  // query) and, when already inside that section, also flip the local view —
  // the section change alone is a no-op then.
  const enter = (target: "askai" | "search", query: string) => {
    openWithQuery(target, query);
    setMode(target === "askai" ? "chat" : "search");
  };

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {mode === "entry" && <EntryView onEnter={enter} />}
      {mode === "search" && <SearchView />}
      {mode === "chat" && <ChatView />}
    </div>
  );
}

/** Centered input with two destinations: file search and AI chat. Both jump
 * to their dedicated sidebar tabs, carrying the typed query along. */
function EntryView({
  onEnter,
}: {
  onEnter: (target: "askai" | "search", query: string) => void;
}) {
  const { t } = useTranslation();
  const snapshot = useLlmStore((s) => s.snapshot);
  const isLoaded = useLlmStore((s) => s.isLoaded);
  const loadConfig = useLlmStore((s) => s.loadConfig);
  const [query, setQuery] = useState("");
  const usable = hasUsableProvider(snapshot);

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  return (
    <div className="flex flex-1 flex-col items-center justify-center px-8">
      <h2 className="text-2xl font-bold text-foreground">{t("home.title")}</h2>
      <p className="mt-1 text-muted-foreground">{t("home.welcome")}</p>

      <div className="mt-6 w-full max-w-xl">
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && query.trim()) {
              if (e.shiftKey) onEnter("askai", query);
              else onEnter("search", query);
            }
          }}
          placeholder={t("home.inputPlaceholder")}
          className="w-full rounded-xl border border-border bg-background px-4 py-3 text-sm text-foreground shadow-sm focus:outline-none focus:ring-2 focus:ring-ring"
        />
        <div className="mt-3 flex justify-center gap-2">
          <button
            onClick={() => onEnter("search", query)}
            className="rounded-lg border border-border bg-background px-4 py-2 text-sm text-foreground hover:bg-accent"
          >
            {t("home.searchAction")}
          </button>
          <button
            onClick={() => onEnter("askai", query)}
            className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90"
          >
            {t("home.askAiAction")}
          </button>
        </div>
        {query.trim() && usable && (
          <p className="mt-2 text-center text-xs text-muted-foreground">
            {t("home.askWithModel", {
              model: defaultModel(snapshot)?.displayName ?? "",
            })}
          </p>
        )}
        <p className="mt-3 text-center text-xs text-muted-foreground">
          {t("home.privacyNotice")}
        </p>
      </div>

      {isLoaded && !usable && (
        <div className="mt-8 w-full max-w-xl rounded-lg border border-dashed border-border bg-card p-4 text-center">
          <p className="text-sm text-muted-foreground">{t("home.noModelGuide")}</p>
          <button
            onClick={() => useNavigationStore.getState().setActiveSection("settings")}
            className="mt-2 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90"
          >
            {t("home.goToSettings")}
          </button>
        </div>
      )}
    </div>
  );
}

/** Global search across the logged-in accounts' drives. Reachable from the
 * sidebar tab; consumes a pending query handed over from the home entry box. */
function SearchView() {
  const { t } = useTranslation();
  const accounts = useAuthStore((s) => s.accounts);
  const openPreviewTab = useTabStore((s) => s.openPreviewTab);
  const setActiveSection = useNavigationStore((s) => s.setActiveSection);
  // Consumed in an effect (not a state initializer) so StrictMode's double
  // invocation cannot swallow the pending query.
  const [submitted, setSubmitted] = useState("");
  const [text, setText] = useState("");
  const [groups, setGroups] = useState<SearchResultGroup[] | null>([]);
  // Result filters: source account, file kind, and modification date.
  const [sourceFilter, setSourceFilter] = useState("all");
  const [typeFilter, setTypeFilter] = useState<SearchTypeFilter>("all");
  const [dateFilter, setDateFilter] = useState<SearchDateFilter>("all");

  useEffect(() => {
    const pending = useNavigationStore.getState().consumePendingQuery();
    if (pending.trim()) setSubmitted(pending);
  }, []);

  useEffect(() => {
    if (!submitted.trim()) {
      // An empty Graph search returns everything; keep the page empty instead.
      setGroups([]);
      return;
    }
    let cancelled = false;
    setGroups(null);
    const run = async () => {
      const results = await Promise.all(
        accounts.map(async (account) => {
          try {
            const items = await searchFiles(
              account.driveId,
              submitted,
              "global",
              account.cloudType
            );
            return {
              accountName: account.alias || account.displayName,
              items,
              driveId: account.driveId,
              cloudEnv: account.cloudType,
              homeAccountId: account.homeAccountId,
            };
          } catch {
            return {
              accountName: account.alias || account.displayName,
              items: [] as DriveItem[],
              driveId: account.driveId,
              cloudEnv: account.cloudType,
              homeAccountId: account.homeAccountId,
            };
          }
        })
      );
      if (!cancelled) setGroups(results.filter((g) => g.items.length > 0));
    };
    run();
    return () => {
      cancelled = true;
    };
  }, [accounts, submitted]);

  const openItem = (group: SearchResultGroup, item: DriveItem) => {
    openPreviewTab(item, group.driveId, group.cloudEnv, group.homeAccountId);
    setActiveSection("files");
  };

  // Client-side filtering over the raw results per search.
  const filteredGroups = (groups ?? [])
    .filter(
      (g) =>
        sourceFilter === "all" || `${g.homeAccountId}:${g.cloudEnv}` === sourceFilter
    )
    .map((g) => ({
      ...g,
      items: g.items.filter(
        (item) =>
          (typeFilter === "all" || fileKind(item) === typeFilter) &&
          withinDateRange(item.lastModified, dateFilter)
      ),
    }))
    .filter((g) => g.items.length > 0);

  const sourceOptions = [
    { value: "all", label: t("home.filterAll") },
    ...accounts.map((a) => ({
      value: `${a.homeAccountId}:${a.cloudType}`,
      label: a.alias || a.displayName,
    })),
  ];
  const typeOptions: { value: SearchTypeFilter; label: string }[] = [
    { value: "all", label: t("home.filterAll") },
    { value: "folder", label: t("home.typeFolder") },
    { value: "doc", label: t("home.typeDoc") },
    { value: "sheet", label: t("home.typeSheet") },
    { value: "slide", label: t("home.typeSlide") },
    { value: "image", label: t("home.typeImage") },
    { value: "other", label: t("home.typeOther") },
  ];
  const dateOptions: { value: SearchDateFilter; label: string }[] = [
    { value: "all", label: t("home.dateAll") },
    { value: "today", label: t("home.dateToday") },
    { value: "week", label: t("home.dateWeek") },
    { value: "month", label: t("home.dateMonth") },
    { value: "year", label: t("home.dateYear") },
  ];

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="pb-3">
        <div className="flex gap-2">
          <input
            type="text"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") setSubmitted(text.trim());
            }}
            placeholder={t("home.searchTabPlaceholder")}
            className="flex-1 rounded-lg border border-border bg-background px-3 py-2 text-sm text-foreground focus:outline-none focus:ring-2 focus:ring-ring"
          />
          <button
            onClick={() => setSubmitted(text.trim())}
            disabled={!text.trim()}
            className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          >
            {t("home.searchAction")}
          </button>
        </div>
        {submitted && (
          <p className="mt-2 text-xs text-muted-foreground">
            {t("home.searchResults", { query: submitted })}
          </p>
        )}
      </div>
      {submitted && groups !== null && groups.length > 0 && (
        <div className="flex items-center gap-1 pb-2">
          <FilterPicker
            icon="🗂"
            fallbackLabel={t("home.filterSource")}
            options={sourceOptions}
            value={sourceFilter}
            onChange={setSourceFilter}
          />
          <FilterPicker
            icon="📄"
            fallbackLabel={t("home.filterType")}
            options={typeOptions}
            value={typeFilter}
            onChange={(v) => setTypeFilter(v as SearchTypeFilter)}
          />
          <FilterPicker
            icon="⏱"
            fallbackLabel={t("home.filterDate")}
            options={dateOptions}
            value={dateFilter}
            onChange={(v) => setDateFilter(v as SearchDateFilter)}
          />
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto">
        {submitted && groups === null && (
          <p className="p-4 text-sm text-muted-foreground">{t("home.searching")}</p>
        )}
        {groups !== null && groups.length === 0 && submitted && (
          <p className="p-4 text-sm text-muted-foreground">{t("home.noResults")}</p>
        )}
        {groups !== null && groups.length > 0 && filteredGroups.length === 0 && (
          <p className="p-4 text-sm text-muted-foreground">
            {t("home.noFilterResults")}
          </p>
        )}
        {!submitted && (
          <p className="p-4 text-sm text-muted-foreground">{t("home.searchTabEmpty")}</p>
        )}
        {filteredGroups.map((group) => (
          <div key={group.homeAccountId} className="mb-4">
            <h3 className="px-1 pb-1 text-sm font-medium text-muted-foreground">
              {group.accountName}
            </h3>
            <ul>
              {group.items.map((item) => (
                <li key={item.id}>
                  <button
                    onClick={() => openItem(group, item)}
                    className="w-full rounded-md px-3 py-2 text-left hover:bg-accent"
                  >
                    <span className="block truncate text-sm text-foreground">
                      {item.name}
                    </span>
                    <span className="block truncate text-xs text-muted-foreground">
                      {item.parentReference?.path ?? ""}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
    </div>
  );
}

/** Streaming chat with a model selector. */
function ChatView() {
  const { t } = useTranslation();
  const snapshot = useLlmStore((s) => s.snapshot);
  const models = allModels(snapshot);
  const fallbackDefault = defaultModel(snapshot);

  const [selected, setSelected] = useState<string>("");
  const accounts = useAuthStore((s) => s.accounts);
  // Last selected M365 accounts persist across sessions; null means "all".
  const [selectedAccountKeys, setSelectedAccountKeys] = useState<string[] | null>(
    loadStoredAccountKeys
  );
  const [effort, setEffort] = useState<ReasoningEffort>(loadReasoningEffort);

  useEffect(() => {
    try {
      localStorage.setItem(ACCOUNT_SELECTION_KEY, JSON.stringify(selectedAccountKeys));
    } catch {
      // Private mode etc.; selection just won't persist.
    }
  }, [selectedAccountKeys]);
  useEffect(() => {
    try {
      localStorage.setItem(REASONING_EFFORT_KEY, effort);
    } catch {
      // Ignore; effort just won't persist.
    }
  }, [effort]);

  const chatAccounts = accounts.map((a) => ({
    homeAccountId: a.homeAccountId,
    cloudType: a.cloudType,
    driveId: a.driveId,
    displayName: a.displayName,
    alias: a.alias,
    label: a.alias || a.displayName,
  }));
  const allAccountKeys = chatAccounts.map((a) => `${a.homeAccountId}:${a.cloudType}`);
  const contextAccountKeys = new Set(
    selectedAccountKeys === null
      ? allAccountKeys
      : allAccountKeys.filter((k) => selectedAccountKeys.includes(k))
  );
  const contextAccounts = chatAccounts.filter((a) =>
    contextAccountKeys.has(`${a.homeAccountId}:${a.cloudType}`)
  );
  const [entries, setEntries] = useState<ChatEntry[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const activeRequestId = useRef<string | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const pendingSentRef = useRef(false);
  const openPreviewTab = useTabStore((s) => s.openPreviewTab);
  const setActiveSection = useNavigationStore((s) => s.setActiveSection);
  // Chat history: the conversation id is kept in a ref so the event listener
  // (mounted once) can persist assistant replies without re-subscribing.
  const conversationIdRef = useRef("");
  const [historyReady, setHistoryReady] = useState(false);
  // Left conversation sidebar: list, active selection and title filter.
  const [conversations, setConversations] = useState<ChatConversationMeta[]>([]);
  const [activeId, setActiveId] = useState("");
  const [listFilter, setListFilter] = useState("");
  const [listCollapsed, setListCollapsed] = useState(readChatListCollapsed);
  const toggleListCollapsed = () => {
    setListCollapsed((value) => {
      const next = !value;
      try {
        localStorage.setItem(CHAT_LIST_COLLAPSED_KEY, next ? "1" : "0");
      } catch {
        // Private mode etc.; collapse just won't persist.
      }
      return next;
    });
  };
  const refreshList = useCallback(() => {
    chatListConversations()
      .then((items) =>
        setConversations(
          [...items].sort((a, b) => b.updatedAt - a.updatedAt)
        )
      )
      .catch(() => setConversations([]));
  }, []);

  const appendToHistory = (message: StoredChatMessage) => {
    if (!conversationIdRef.current) return;
    chatAppendMessage(conversationIdRef.current, message).catch(() => undefined);
  };

  const loadConversation = async (id: string | null) => {
    try {
      const detail = await chatOpenConversation(id);
      conversationIdRef.current = detail.id;
      setActiveId(detail.id);
      setEntries(
        detail.messages.map((m) => ({
          role: m.role,
          content: m.content,
          reasoning: m.reasoning ?? undefined,
          contextFiles: m.contextFiles.length > 0 ? m.contextFiles : undefined,
        }))
      );
      return true;
    } catch {
      // History unavailable (e.g. DB locked) — start with an empty view.
      conversationIdRef.current = "";
      setActiveId("");
      setEntries([]);
      return false;
    }
  };

  // Load the latest conversation before any auto-submit — except when
  // arriving from the home entry box with a typed question: that starts a
  // brand-new conversation so the question never lands in an old thread.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const hasPending =
        useNavigationStore.getState().pendingQuery.trim().length > 0;
      try {
        if (hasPending) {
          const id = await chatNewConversation();
          if (cancelled) return;
          conversationIdRef.current = id;
          setEntries([]);
        } else {
          await loadConversation(null);
        }
      } catch {
        // History unavailable (e.g. DB locked) — start with an empty view.
        conversationIdRef.current = "";
        setEntries([]);
      }
      if (!cancelled) {
        setHistoryReady(true);
        refreshList();
      }
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const usable = hasUsableProvider(snapshot);
  const selectedValue = selected || (fallbackDefault
    ? `${fallbackDefault.providerId}:${fallbackDefault.modelId}`
    : "");

  useEffect(() => {
    // StrictMode mounts effects twice; the first listen promise may resolve
    // after cleanup ran, so guard with a cancelled flag or the orphan
    // listener stays registered and every delta gets appended twice.
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;
    listen<LlmChatEvent>(LLM_CHAT_EVENT, (event) => {
      const payload = event.payload;
      if (payload.requestId !== activeRequestId.current) return;
      setEntries((prev) => {
        const next = [...prev];
        const last = next[next.length - 1];
        if (payload.kind === "delta" && payload.delta) {
          if (last?.isStreaming) {
            next[next.length - 1] = { ...last, content: last.content + payload.delta };
          } else {
            next.push({ role: "assistant", content: payload.delta, isStreaming: true });
          }
        } else if (payload.kind === "reasoning" && payload.delta) {
          if (last?.isStreaming) {
            next[next.length - 1] = {
              ...last,
              reasoning: (last.reasoning ?? "") + payload.delta,
              // First content after reasoning stops the thinking clock.
              reasoningMs:
                last.reasoningStartedAt != null && last.reasoningMs == null
                  ? Date.now() - last.reasoningStartedAt
                  : last.reasoningMs,
            };
          } else {
            next.push({
              role: "assistant",
              content: "",
              reasoning: payload.delta,
              isStreaming: true,
              reasoningStartedAt: Date.now(),
            });
          }
        } else if (payload.kind === "done") {
          if (last?.isStreaming) {
            next[next.length - 1] = {
              ...last,
              isStreaming: false,
              reasoningMs:
                last.reasoningStartedAt != null && last.reasoningMs == null
                  ? Date.now() - last.reasoningStartedAt
                  : last.reasoningMs,
            };
          }
          // Persist the finished assistant reply (also covers partial content
          // left by a user-initiated stop, which ends with a done event).
          const finished = next[next.length - 1];
          if (
            finished?.role === "assistant" &&
            (finished.content || finished.reasoning)
          ) {
            appendToHistory({
              role: "assistant",
              content: finished.content,
              reasoning: finished.reasoning ?? null,
              contextFiles: [],
              createdAt: Date.now(),
            });
            refreshList();
          }
          setBusy(false);
          activeRequestId.current = null;
        } else if (payload.kind === "error") {
          if (last?.isStreaming) next[next.length - 1] = { ...last, isStreaming: false };
          next.push({
            role: "assistant",
            content: "",
            isStreaming: false,
            error: payload.message ?? t("home.chatError"),
          });
          setBusy(false);
          activeRequestId.current = null;
        }
        return next;
      });
    }).then((fn) => {
      if (cancelled) {
        fn();
        return;
      }
      unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [t]);
  // Auto-scroll as content streams in.
  useEffect(() => {
    listRef.current?.scrollTo({ top: listRef.current.scrollHeight });
  }, [entries]);

  // `text` override lets the pending-query effect submit before `input`
  // state would have propagated through a render.
  const send = async (textOverride?: string) => {
    const text = (textOverride ?? input).trim();
    if (!text || busy) return;
    const [providerId, modelId] = selectedValue.split(":");
    if (!providerId || !modelId) return;

    const history: LlmChatMessage[] = entries
      .filter((e) => !e.error)
      .map((e) => ({ role: e.role, content: e.content }));
    const messages = [...history, { role: "user", content: text } as LlmChatMessage];

    setInput("");
    setEntries((prev) => [...prev, { role: "user", content: text }]);
    setBusy(true);

    // Generate the id up front so deltas that arrive before the invoke
    // resolves are not filtered out.
    const requestId = newLlmRequestId();
    activeRequestId.current = requestId;
    try {
      const contextFiles = await gatherCloudContext(text, contextAccounts);
      // Attach the gathered files to the user's bubble as citation chips and
      // persist the user message with them.
      setEntries((prev) => {
        const last = prev[prev.length - 1];
        if (last?.role === "user" && last.content === text) {
          return [...prev.slice(0, -1), { ...last, contextFiles }];
        }
        return prev;
      });
      appendToHistory({
        role: "user",
        content: text,
        reasoning: null,
        contextFiles,
        createdAt: Date.now(),
      });
      refreshList();
      await llmChat(
        providerId,
        modelId,
        messages,
        requestId,
        contextFiles.map(toLlmContextFile),
        effort
      );
    } catch (e) {
      setBusy(false);
      activeRequestId.current = null;
      setEntries((prev) => [
        ...prev,
        { role: "assistant", content: "", error: formatAppError(e) },
      ]);
    }
  };

  const stop = () => {
    if (activeRequestId.current) {
      cancelLlmChat(activeRequestId.current).catch(() => undefined);
      activeRequestId.current = null;
    }
    setEntries((prev) => {
      const last = prev[prev.length - 1];
      if (last?.isStreaming) {
        return [...prev.slice(0, -1), { ...last, isStreaming: false }];
      }
      return prev;
    });
    setBusy(false);
  };

  // Start a fresh conversation; the old one stays in history.
  const startNewChat = async () => {
    if (busy) return;
    try {
      const id = await chatNewConversation();
      conversationIdRef.current = id;
      setActiveId(id);
      setEntries([]);
      setInput("");
      refreshList();
    } catch {
      // Keep the current conversation on failure.
    }
  };

  // Arriving from the home entry box with a question typed: submit it once,
  // immediately — but only after chat history finished loading, or the
  // history load would overwrite the just-sent message. The effect re-runs
  // when historyReady flips (it is always false on mount, since the history
  // load is async — an empty deps array here would never fire the submit).
  // Consuming the pending query here (an effect, not a state initializer)
  // keeps it safe under StrictMode's double invocation; when no usable model
  // is configured the text stays in the input box instead.
  useEffect(() => {
    if (!historyReady || pendingSentRef.current) return;
    const pending = useNavigationStore.getState().consumePendingQuery();
    if (!pending.trim()) return;
    pendingSentRef.current = true;
    setInput(pending);
    if (hasUsableProvider(useLlmStore.getState().snapshot)) {
      send(pending);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [historyReady]);

  if (!usable) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-3 p-8">
        <p className="text-sm text-muted-foreground">{t("home.noModelGuide")}</p>
        <button
          onClick={() => useNavigationStore.getState().setActiveSection("settings")}
          className="rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90"
        >
          {t("home.goToSettings")}
        </button>
      </div>
    );
  }

  const filteredConversations = listFilter.trim()
    ? conversations.filter((c) =>
        (c.title || t("home.newChat"))
          .toLowerCase()
          .includes(listFilter.trim().toLowerCase())
      )
    : conversations;

  return (
    <div className="flex h-full min-h-0">
      {/* Conversation sidebar (Gemini-style): new chat, title search, recents */}
      {!listCollapsed && (
      <aside className="flex w-64 shrink-0 flex-col border-r border-border bg-card">
        <div className="flex items-center justify-end px-2 pt-2">
          <button
            onClick={toggleListCollapsed}
            className="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
            aria-label={t("nav.collapse")}
            title={t("nav.collapse")}
          >
            <PanelLeftClose className="h-4 w-4" />
          </button>
        </div>
        <div className="space-y-2 px-2 pb-2">
          <button
            onClick={startNewChat}
            disabled={busy}
            className="flex w-full items-center gap-2 rounded-lg border border-border px-3 py-2 text-left text-sm text-foreground hover:bg-accent disabled:opacity-50"
          >
            <span aria-hidden>✚</span>
            {t("home.newChat")}
          </button>
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
            <input
              value={listFilter}
              onChange={(e) => setListFilter(e.target.value)}
              placeholder={t("home.chatSearch")}
              className="w-full rounded-lg border border-border bg-background py-1.5 pl-8 pr-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring"
            />
          </div>
        </div>
        <p className="px-3 pb-1 text-xs font-medium text-muted-foreground">
          {t("home.recentChats")}
        </p>
        <div className="min-h-0 flex-1 overflow-auto px-2 pb-2">
          {filteredConversations.length === 0 ? (
            <p className="px-2 py-4 text-xs text-muted-foreground">
              {listFilter.trim() ? t("home.noChatMatches") : t("home.historyEmpty")}
            </p>
          ) : (
            <ul className="space-y-0.5">
              {filteredConversations.map((conversation) => {
                const isActive = conversation.id === activeId;
                return (
                  <li key={conversation.id} className="group relative">
                    <button
                      onClick={() => loadConversation(conversation.id)}
                      className={`w-full rounded-md px-2.5 py-2 text-left transition-colors ${
                        isActive
                          ? "bg-accent text-accent-foreground"
                          : "hover:bg-accent/60"
                      }`}
                    >
                      <span className="block truncate pr-5 text-sm text-foreground">
                        {conversation.title || t("home.newChat")}
                      </span>
                      <span className="block text-[10px] text-muted-foreground">
                        {new Date(conversation.updatedAt * 1000).toLocaleString()}
                      </span>
                    </button>
                    <button
                      title={t("llm.delete")}
                      onClick={() => {
                        chatDeleteConversation(conversation.id)
                          .catch(() => undefined)
                          .finally(() => {
                            refreshList();
                            if (conversation.id === activeId) startNewChat();
                          });
                      }}
                      className="absolute right-1.5 top-2 rounded p-0.5 text-xs text-muted-foreground opacity-0 transition-opacity hover:text-destructive group-hover:opacity-100"
                    >
                      ×
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </aside>
      )}

      <div className="relative flex min-w-0 flex-1 flex-col overflow-hidden">
      {listCollapsed && (
        <button
          onClick={toggleListCollapsed}
          className="absolute left-2 top-2 z-10 rounded-md border border-border bg-card p-1.5 text-muted-foreground shadow-sm hover:bg-accent hover:text-foreground transition-colors"
          aria-label={t("nav.expand")}
          title={t("nav.expand")}
        >
          <PanelLeftOpen className="h-4 w-4" />
        </button>
      )}
      <div ref={listRef} className="min-h-0 flex-1 space-y-3 overflow-auto pb-2">
        {entries.length === 0 && (
          <p className="pt-8 text-center text-sm text-muted-foreground">
            {t("home.chatEmpty")}
          </p>
        )}
        {entries.map((entry, index) => (
          <div
            key={index}
            className={`flex flex-col ${entry.role === "user" ? "items-end" : "items-start"}`}
          >
            <div
              className={`max-w-[80%] min-w-0 overflow-hidden whitespace-pre-wrap break-words rounded-xl px-3 py-2 text-sm ${
                entry.role === "user"
                  ? "bg-primary text-primary-foreground"
                  : "bg-muted/60 text-foreground"
              }`}
            >
              {entry.error ? (
                <span className="text-destructive">
                  {t("home.chatErrorPrefix")}
                  {entry.error}
                  <button
                    onClick={() => setInput(entries[index - 1]?.content ?? "")}
                    className="ml-2 underline hover:opacity-80"
                  >
                    {t("home.retry")}
                  </button>
                </span>
              ) : (
                <>
                  {entry.reasoning && (
                    <ReasoningBlock
                      text={entry.reasoning}
                      thinking={!!entry.isStreaming && !entry.content}
                      durationMs={entry.reasoningMs}
                    />
                  )}
                  {entry.role === "assistant" ? (
                    <>
                      <Markdown text={entry.content} />
                      {entry.isStreaming && entry.content && (
                        <span className="animate-pulse">▍</span>
                      )}
                    </>
                  ) : (
                    entry.content
                  )}
                </>
              )}
            </div>
            {/* Citation chips shown under the ANSWER, sourced from the
                question they grounded (also works for history-loaded turns). */}
            {(() => {
              const chips =
                entry.role === "assistant"
                  ? entries
                      .slice(0, index)
                      .reverse()
                      .find((e) => e.role === "user")?.contextFiles
                  : undefined;
              if (!chips || chips.length === 0) return null;
              return (
                <div className="mt-1 flex max-w-[80%] flex-wrap items-center gap-1">
                  <span className="text-xs text-muted-foreground">
                    {t("home.contextFiles")}
                  </span>
                  {chips.map((file) => (
                    <button
                      key={`${file.homeAccountId}:${file.item.id}`}
                      onClick={() => {
                        openPreviewTab(file.item, file.driveId, file.cloudEnv, file.homeAccountId);
                        setActiveSection("files");
                      }}
                      title={file.excerpt ? t("home.contextFileWithExcerpt") : file.path}
                      className={`max-w-56 truncate rounded-full border px-2 py-0.5 text-xs ${
                        file.excerpt
                          ? "border-primary/40 bg-primary/10 text-primary"
                          : "border-border text-muted-foreground hover:bg-accent hover:text-foreground"
                      }`}
                    >
                      📄 {file.item.name}
                    </button>
                  ))}
                </div>
              );
            })()}
          </div>
        ))}
        {/* Waiting for the model's first token: shown below the question it
            belongs to, on the assistant's side of the conversation. */}
        {busy && entries[entries.length - 1]?.role === "user" && (
          <div className="flex justify-start">
            <div className="flex items-center gap-1.5 rounded-xl bg-muted/60 px-3 py-2 text-sm text-muted-foreground">
              <span className="flex gap-1">
                <span className="size-1.5 animate-bounce rounded-full bg-current [animation-delay:0ms]" />
                <span className="size-1.5 animate-bounce rounded-full bg-current [animation-delay:150ms]" />
                <span className="size-1.5 animate-bounce rounded-full bg-current [animation-delay:300ms]" />
              </span>
              {t("home.thinking")}
            </div>
          </div>
        )}
      </div>

      <div className="rounded-2xl border border-border bg-card p-2 shadow-sm">
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
          rows={2}
          placeholder={t("home.chatPlaceholder")}
          className="max-h-32 w-full resize-y border-0 bg-transparent px-2 py-1.5 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none"
        />
        <div className="flex items-center justify-between gap-2 pt-1">
          <div className="flex items-center gap-1">
            <AccountPicker
              accounts={chatAccounts}
              selectedKeys={selectedAccountKeys}
              onChange={setSelectedAccountKeys}
            />
          </div>
          <div className="flex items-center gap-1">
            <ModelPicker
              models={models}
              value={selectedValue}
              onChange={setSelected}
              defaultRef={fallbackDefault}
            />
            <EffortPicker value={effort} onChange={setEffort} />
            {busy ? (
              <button
                onClick={stop}
                title={t("home.stop")}
                className="ml-1 flex size-8 items-center justify-center rounded-full bg-muted text-foreground hover:bg-accent"
              >
                ■
              </button>
            ) : (
              <button
                onClick={() => send()}
                disabled={!input.trim()}
                title={t("home.send")}
                className="ml-1 flex size-8 items-center justify-center rounded-full bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40"
              >
                ↑
              </button>
            )}
          </div>
        </div>
      </div>
      <p className="pt-1 text-center text-xs text-muted-foreground">
        {t("home.privacyNotice")}
      </p>
      </div>
    </div>
  );
}
