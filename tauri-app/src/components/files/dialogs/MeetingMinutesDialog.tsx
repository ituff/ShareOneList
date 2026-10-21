import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { dirname, downloadDir, join } from "@tauri-apps/api/path";
import { save } from "@tauri-apps/plugin-dialog";
import { AlertCircle, ClipboardList, Download, Loader2, Sparkles } from "lucide-react";
import {
  cancelLlmChat,
  exportMeetingMinutesDocx,
  formatAppError,
  getLlmConfig,
  llmMeetingMinutes,
  newLlmRequestId,
} from "../../../lib/tauri";
import type { LlmChatEvent } from "../../../lib/types";
import { Markdown } from "../../home/Markdown";
import { useSettingsStore } from "../../../stores/settingsStore";
import { useToastStore } from "../../../stores/toastStore";

const LLM_CHAT_EVENT = "llm-chat-event";

/** Outcome of collecting the meeting transcript (see PreviewPage). */
export type MinutesTranscriptResult =
  | { kind: "ok"; script: string }
  | { kind: "none" }
  | { kind: "needPlayback" }
  | { kind: "error"; message: string };

interface MeetingMinutesDialogProps {
  /** Recording base name; doubles as the .docx document title. */
  meetingTitle: string;
  /** Collects the transcript script (player capture first, drive fallback). */
  loadTranscript: () => Promise<MinutesTranscriptResult>;
  onClose: () => void;
}

type Phase = "transcript" | "generating" | "done" | "error";

/**
 * Modal dialog that generates AI meeting minutes from the recording's
 * transcript with the default LLM: streams the markdown in place, then lets
 * the user export it as a Word document.
 */
export function MeetingMinutesDialog({
  meetingTitle,
  loadTranscript,
  onClose,
}: MeetingMinutesDialogProps) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const lastDownloadPath = useSettingsStore((s) => s.lastDownloadPath);
  const setLastDownloadPath = useSettingsStore((s) => s.setLastDownloadPath);
  const [phase, setPhase] = useState<Phase>("transcript");
  const [minutes, setMinutes] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isSavingDocx, setIsSavingDocx] = useState(false);
  const activeRequestIdRef = useRef<string | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  // True once any content delta arrived for the active request; a stale true
  // from a previous attempt would mask the "model returned nothing" case.
  const streamedRef = useRef(false);
  // StrictMode mounts effects twice; without this the minutes request fires
  // twice and one stream runs orphaned on the backend.
  const startedRef = useRef(false);

  const handleClose = useCallback(() => {
    // A stream still in flight is cancelled; partial minutes are discarded.
    const id = activeRequestIdRef.current;
    if (id) {
      cancelLlmChat(id).catch(() => undefined);
      activeRequestIdRef.current = null;
    }
    onClose();
  }, [onClose]);

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !isSavingDocx) handleClose();
    };
    document.addEventListener("keydown", handleEscape);
    return () => document.removeEventListener("keydown", handleEscape);
  }, [handleClose, isSavingDocx]);

  // Full pipeline: transcript → default-model minutes, streaming in place.
  const run = useCallback(async () => {
    setPhase("transcript");
    setMinutes("");
    setError(null);

    try {
      // Fail fast with a targeted message when no default model is set.
      const snapshot = await getLlmConfig();
      if (!snapshot.config.defaultModel) {
        setPhase("error");
        setError(t("preview.minutesNoDefaultModel"));
        return;
      }
    } catch (e) {
      setPhase("error");
      setError(formatAppError(e));
      return;
    }

    let script: string;
    try {
      const result = await loadTranscript();
      if (result.kind === "error") {
        setPhase("error");
        setError(result.message);
        return;
      }
      if (result.kind === "none" || result.kind === "needPlayback") {
        setPhase("error");
        setError(
          result.kind === "needPlayback"
            ? t("preview.transcriptNeedPlayback")
            : t("preview.transcriptNotFound")
        );
        return;
      }
      script = result.script;
    } catch (e) {
      setPhase("error");
      setError(formatAppError(e));
      return;
    }

    setPhase("generating");
    const requestId = newLlmRequestId();
    activeRequestIdRef.current = requestId;
    streamedRef.current = false;
    try {
      // Resolves as soon as the backend task is spawned; the minutes stream
      // in through the event listener below (done/error set the final phase).
      await llmMeetingMinutes(requestId, meetingTitle, script);
    } catch (e) {
      // Startup failure (no default model, missing key, empty script).
      if (activeRequestIdRef.current === requestId) {
        activeRequestIdRef.current = null;
        setPhase((prev) => (prev === "generating" ? "error" : prev));
        setError(formatAppError(e));
      }
    }
  }, [loadTranscript, meetingTitle, t]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;

    // StrictMode double-mount guard (see HomePage): an orphan listener would
    // append every delta twice.
    listen<LlmChatEvent>(LLM_CHAT_EVENT, (event) => {
      const payload = event.payload;
      if (payload.requestId !== activeRequestIdRef.current) return;
      if (payload.kind === "delta" && payload.delta) {
        streamedRef.current = true;
        setMinutes((prev) => prev + payload.delta);
      } else if (payload.kind === "reasoning") {
        // Chain-of-thought of reasoning models is not part of the minutes.
      } else if (payload.kind === "done") {
        activeRequestIdRef.current = null;
        if (streamedRef.current) {
          setPhase("done");
        } else {
          setError(t("preview.minutesEmpty"));
          setPhase("error");
        }
      } else if (payload.kind === "error") {
        activeRequestIdRef.current = null;
        setError(`${t("preview.minutesFailed")}: ${payload.message ?? ""}`);
        setPhase("error");
      }
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [t]);

  // Start once on mount; retries re-run the whole pipeline.
  useEffect(() => {
    if (startedRef.current) return;
    startedRef.current = true;
    run();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Keep the streaming minutes in view as they grow.
  useEffect(() => {
    if (phase === "generating" && contentRef.current) {
      contentRef.current.scrollTop = contentRef.current.scrollHeight;
    }
  }, [minutes, phase]);

  const handleDownloadDocx = useCallback(async () => {
    if (!minutes.trim() || isSavingDocx) return;
    try {
      const dir = lastDownloadPath ?? (await downloadDir());
      const suffix = t("preview.minutesFileSuffix");
      const selected = await save({
        defaultPath: await join(dir, `${meetingTitle}_${suffix}.docx`),
        filters: [{ name: "Word", extensions: ["docx"] }],
      });
      if (!selected) return;
      setLastDownloadPath(await dirname(selected));
      setIsSavingDocx(true);
      await exportMeetingMinutesDocx(selected, meetingTitle, minutes.trim());
      addToast("success", t("preview.minutesSaved"));
    } catch (e) {
      addToast("error", formatAppError(e));
    } finally {
      setIsSavingDocx(false);
    }
  }, [minutes, isSavingDocx, lastDownloadPath, meetingTitle, setLastDownloadPath, addToast, t]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={() => {
        if (!isSavingDocx) handleClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="meeting-minutes-title"
    >
      <div
        className="flex h-[80vh] w-full max-w-2xl flex-col rounded-lg bg-card p-6 shadow-lg border border-border"
        onClick={(e) => e.stopPropagation()}
      >
        <h3
          id="meeting-minutes-title"
          className="mb-3 flex items-center gap-2 text-lg font-semibold text-foreground"
          title={meetingTitle}
        >
          <ClipboardList className="h-5 w-5 shrink-0 text-primary" />
          <span className="truncate">{t("preview.minutesTitle")}</span>
        </h3>

        <div
          ref={contentRef}
          className="min-h-0 flex-1 overflow-y-auto rounded-md bg-background px-4 py-3"
        >
          {phase === "transcript" && (
            <div className="flex h-full flex-col items-center justify-center gap-3 p-8">
              <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
              <p className="text-sm text-muted-foreground">
                {t("preview.minutesCollecting")}
              </p>
            </div>
          )}

          {phase === "generating" && (
            <div className="space-y-3">
              <div className="flex items-center gap-2 text-xs text-muted-foreground">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t("preview.minutesGenerating")}
              </div>
              {minutes ? (
                <Markdown text={minutes} />
              ) : (
                <p className="text-sm text-muted-foreground/70">
                  {t("preview.minutesWaiting")}
                </p>
              )}
            </div>
          )}

          {phase === "done" && <Markdown text={minutes} />}

          {phase === "error" && (
            <div className="flex h-full flex-col items-center justify-center gap-3 p-8">
              <AlertCircle className="h-8 w-8 text-destructive" />
              <p className="max-w-md text-center text-sm text-muted-foreground">{error}</p>
              <button
                onClick={run}
                className="inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
              >
                <Sparkles className="h-3.5 w-3.5" />
                {t("preview.minutesRetry")}
              </button>
            </div>
          )}
        </div>

        <div className="mt-4 flex items-center justify-end gap-2">
          <button
            onClick={handleDownloadDocx}
            disabled={phase !== "done" || isSavingDocx || !minutes.trim()}
            className="inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {isSavingDocx ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Download className="h-3.5 w-3.5" />
            )}
            {t("preview.minutesDownload")}
          </button>
          <button
            onClick={handleClose}
            className="rounded-md px-4 py-2 text-sm font-medium text-muted-foreground hover:bg-accent transition-colors"
          >
            {t("dialogs.close")}
          </button>
        </div>
      </div>
    </div>
  );
}
