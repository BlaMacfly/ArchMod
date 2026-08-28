import { AlertTriangle } from "lucide-react";
import { Modal } from "./Modal";

interface ConfirmDialogProps {
  title: string;
  message: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  return (
    <Modal
      title={title}
      onClose={onCancel}
      footer={
        <>
          <button
            type="button"
            onClick={onCancel}
            className="rounded-lg border border-ink-600 px-4 py-2 text-sm text-mist-300 transition-colors hover:bg-white/5"
          >
            Annuler
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className="rounded-lg bg-brand-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-brand-500 hover:text-ink-950"
          >
            {confirmLabel}
          </button>
        </>
      }
    >
      <p className="flex gap-3 text-sm text-mist-300">
        <AlertTriangle className="mt-0.5 h-5 w-5 shrink-0 text-warn-500" />
        {message}
      </p>
    </Modal>
  );
}
