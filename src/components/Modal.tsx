import { useEffect, type ReactNode } from "react";
import { X } from "lucide-react";
import { useI18n } from "../i18n";

interface ModalProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  width?: string;
}

/** Boîte de dialogue centrée, fermable au clavier (Échap) ou au clic extérieur. */
export function Modal({ title, onClose, children, footer, width }: ModalProps) {
  const { t } = useI18n();
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6 backdrop-blur-sm"
      role="presentation"
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className={[
          "animate-fade-up rounded-card border border-ink-600 bg-ink-850 shadow-2xl",
          width ?? "w-full max-w-lg",
        ].join(" ")}
      >
        <header className="flex items-center justify-between border-b border-ink-700 px-5 py-3.5">
          <h2 className="text-base font-semibold text-mist-100">{title}</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label={t("dialog.close")}
            className="rounded-md p-1 text-mist-400 transition-colors hover:bg-white/5 hover:text-mist-100"
          >
            <X className="h-4 w-4" />
          </button>
        </header>
        <div className="px-5 py-4">{children}</div>
        {footer && (
          <footer className="flex justify-end gap-2 border-t border-ink-700 px-5 py-3.5">
            {footer}
          </footer>
        )}
      </div>
    </div>
  );
}
