import type { ReactNode } from "react";
import { AlertTriangle, Info, XCircle } from "lucide-react";

type Tone = "warn" | "error" | "info";

const TONES: Record<Tone, { border: string; text: string; Icon: typeof Info }> = {
  warn: {
    border: "border-warn-500/30 bg-warn-500/10",
    text: "text-warn-500",
    Icon: AlertTriangle,
  },
  error: {
    border: "border-halt-500/30 bg-halt-500/10",
    text: "text-halt-500",
    Icon: XCircle,
  },
  info: {
    border: "border-brand-500/30 bg-brand-500/10",
    text: "text-brand-400",
    Icon: Info,
  },
};

interface AlertProps {
  tone: Tone;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
}

/** Bandeau d'information affiché sous l'en-tête (dépendances, configuration). */
export function Alert({ tone, title, children, action }: AlertProps) {
  const { border, text, Icon } = TONES[tone];
  return (
    <div
      className={`flex items-start gap-3 border-b px-6 py-3 text-sm ${border}`}
      role="status"
    >
      <Icon className={`mt-0.5 h-4 w-4 shrink-0 ${text}`} />
      <div className="min-w-0 flex-1">
        <p className={`font-medium ${text}`}>{title}</p>
        {children && <div className="mt-0.5 text-mist-400">{children}</div>}
      </div>
      {action}
    </div>
  );
}
