import { X, CheckCircle, AlertCircle, Info } from "lucide-react";
import { useToastStore } from "../../stores/toastStore";
import type { ToastType } from "../../stores/toastStore";

const iconMap: Record<ToastType, React.ReactNode> = {
  success: <CheckCircle className="h-4 w-4 text-green-500" />,
  error: <AlertCircle className="h-4 w-4 text-destructive" />,
  info: <Info className="h-4 w-4 text-blue-500" />,
};

const bgMap: Record<ToastType, string> = {
  success: "border-green-500/30 bg-green-500/5",
  error: "border-destructive/30 bg-destructive/5",
  info: "border-blue-500/30 bg-blue-500/5",
};

/**
 * Toast container that renders all active toast notifications.
 * Should be placed once at the app root level.
 */
export function ToastContainer() {
  const { toasts, removeToast } = useToastStore();

  if (toasts.length === 0) return null;

  return (
    <div
      className="fixed bottom-4 right-4 z-[100] flex flex-col gap-2"
      aria-live="polite"
      aria-label="Notifications"
    >
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className={`flex items-center gap-2 rounded-lg border px-4 py-3 shadow-md animate-in slide-in-from-right ${bgMap[toast.type]}`}
          role="alert"
        >
          {iconMap[toast.type]}
          <span className="text-sm text-foreground flex-1">{toast.message}</span>
          <button
            onClick={() => removeToast(toast.id)}
            className="text-muted-foreground hover:text-foreground transition-colors"
            aria-label="Dismiss"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      ))}
    </div>
  );
}
