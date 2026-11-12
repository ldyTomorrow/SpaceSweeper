import { useEffect, useRef } from "react";
import type { ReactNode } from "react";
import { TrashIcon } from "./Icons";
import { formatBytes, formatCount } from "../lib/format";

interface DeleteConfirmDialogProps {
  open: boolean;
  count: number;
  totalBytes: number;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
  title?: string;
  notes?: ReactNode[];
  confirmLabel?: string;
}

export function DeleteConfirmDialog({
  open,
  count,
  totalBytes,
  busy,
  onCancel,
  onConfirm,
  title = "确认移入废纸篓？",
  notes,
  confirmLabel = "移入废纸篓",
}: DeleteConfirmDialogProps) {
  const cancelRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onCancel();
    };

    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    document.addEventListener("keydown", handleKeyDown);
    cancelRef.current?.focus();

    return () => {
      document.body.style.overflow = previousOverflow;
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [open, busy, onCancel]);

  if (!open) return null;

  return (
    <div
      className="dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onCancel();
      }}
    >
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="delete-dialog-title"
      >
        <div className="dialog-icon">
          <TrashIcon size={22} />
        </div>

        <h2 id="delete-dialog-title">{title}</h2>
        <p className="dialog-lead">
          将移入废纸篓 <strong>{formatCount(count)}</strong> 项，共{" "}
          <strong>{formatBytes(totalBytes)}</strong>。
        </p>

        <div className="dialog-note">
          {notes ? (
            notes.map((note, index) => <p key={index}>{note}</p>)
          ) : (
            <>
              <p>
                删除后仍可在废纸篓中「放回原处」恢复，不会永久删除；清空废纸篓后才会释放空间。
              </p>
              <p>本次仅处理你勾选的应用缓存，不触碰个人文档、应用设置与系统文件。</p>
            </>
          )}
        </div>

        <div className="dialog-actions">
          <button
            ref={cancelRef}
            type="button"
            className="btn ghost"
            onClick={onCancel}
            disabled={busy}
          >
            取消
          </button>
          <button
            type="button"
            className="btn primary"
            onClick={onConfirm}
            disabled={busy || count === 0}
          >
            {busy ? "正在移入…" : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
