import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { AlertIcon, TrashIcon } from "./Icons";
import { formatBytes, formatCount } from "../lib/format";

export interface DeleteConfirmOptions {
  permanent: boolean;
}

interface DeleteConfirmDialogProps {
  open: boolean;
  count: number;
  totalBytes: number;
  busy: boolean;
  onCancel: () => void;
  onConfirm: (options: DeleteConfirmOptions) => void;
  title?: string;
  notes?: ReactNode[];
  confirmLabel?: string;
  allowPermanent?: boolean;
  requireAcknowledgement?: boolean;
  acknowledgementLabel?: string;
  danger?: boolean;
}

const PERMANENT_PHRASE = "永久删除";

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
  allowPermanent = false,
  requireAcknowledgement = false,
  acknowledgementLabel = "我了解上述后果，并确认这些内容不再需要。",
  danger = false,
}: DeleteConfirmDialogProps) {
  const cancelRef = useRef<HTMLButtonElement>(null);
  const [permanent, setPermanent] = useState(false);
  const [acknowledged, setAcknowledged] = useState(false);
  const [confirmText, setConfirmText] = useState("");

  useEffect(() => {
    if (!open) return;
    setPermanent(false);
    setAcknowledged(false);
    setConfirmText("");
  }, [open]);

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

  const permanentReady = !permanent || confirmText.trim() === PERMANENT_PHRASE;
  const acknowledgementReady = !requireAcknowledgement || acknowledged;
  const canConfirm = count > 0 && permanentReady && acknowledgementReady;

  return (
    <div
      className="dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onCancel();
      }}
    >
      <div
        className={`dialog${danger ? " is-danger" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="delete-dialog-title"
      >
        <div className="dialog-icon">
          {danger || permanent ? <AlertIcon size={22} /> : <TrashIcon size={22} />}
        </div>

        <h2 id="delete-dialog-title">{title}</h2>
        <p className="dialog-lead">
          将处理 <strong>{formatCount(count)}</strong> 项，共{" "}
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
              <p>本次仅处理你勾选的项目，不触碰未选中的内容。</p>
            </>
          )}
          {allowPermanent && permanent && (
            <p className="dialog-danger-text">
              永久删除不会进入废纸篓，也无法使用「放回原处」恢复。
            </p>
          )}
        </div>

        {requireAcknowledgement && (
          <label className="dialog-check">
            <input
              type="checkbox"
              checked={acknowledged}
              onChange={(event) => setAcknowledged(event.target.checked)}
              disabled={busy}
            />
            <span>{acknowledgementLabel}</span>
          </label>
        )}

        {allowPermanent && (
          <div className="dialog-permanent">
            <label className="dialog-check">
              <input
                type="checkbox"
                checked={permanent}
                onChange={(event) => {
                  setPermanent(event.target.checked);
                  if (!event.target.checked) setConfirmText("");
                }}
                disabled={busy}
              />
              <span>永久删除，不进入废纸篓（不可恢复）</span>
            </label>

            {permanent && (
              <label className="dialog-confirm-text">
                <span>
                  输入 <strong>{PERMANENT_PHRASE}</strong> 以继续
                </span>
                <input
                  type="text"
                  value={confirmText}
                  onChange={(event) => setConfirmText(event.target.value)}
                  placeholder={PERMANENT_PHRASE}
                  autoComplete="off"
                  spellCheck={false}
                  disabled={busy}
                />
              </label>
            )}
          </div>
        )}

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
            className={`btn ${danger || permanent ? "danger" : "primary"}`}
            onClick={() => onConfirm({ permanent })}
            disabled={busy || !canConfirm}
          >
            {busy
              ? "正在处理…"
              : permanent
                ? "永久删除"
                : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
