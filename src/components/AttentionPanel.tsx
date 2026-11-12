import { useCallback, useMemo, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  CacheItem,
  DeleteReport,
  PermanentTicket,
  ScanEvent,
  ScanSummary,
} from "../types";
import { formatBytes, formatCount, toErrorMessage } from "../lib/format";
import { ScanResultList } from "./ScanResultList";
import { DeleteConfirmDialog } from "./DeleteConfirmDialog";
import type { DeleteConfirmOptions } from "./DeleteConfirmDialog";
import { AlertIcon, CheckIcon, CloseIcon, ScanIcon, TrashIcon } from "./Icons";

interface AttentionProgress {
  itemsDone: number;
  itemsTotal: number;
  fileCount: number;
  totalBytes: number;
}

const NOTES = [
  "设备符号、Docker 容器与卷、模拟器设备、模型缓存和 iOS 备份都属于需确认内容。",
  "删除后可能无法恢复，部分内容需要重新连接设备或重新下载。",
  "运行中的 Docker、Xcode 或模拟器会被安全守卫自动跳过。",
];

export function AttentionPanel() {
  const [items, setItems] = useState<CacheItem[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [scanning, setScanning] = useState(false);
  const [hasScanned, setHasScanned] = useState(false);
  const [summary, setSummary] = useState<ScanSummary | null>(null);
  const [progress, setProgress] = useState<AttentionProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<DeleteReport | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const scanInFlight = useRef(false);
  const deleteInFlight = useRef(false);

  const selectedItems = useMemo(
    () => items.filter((item) => selected.has(item.id)),
    [items, selected],
  );
  const selectedBytes = useMemo(
    () => selectedItems.reduce((total, item) => total + item.size, 0),
    [selectedItems],
  );
  const sortedItems = useMemo(
    () => [...items].sort((left, right) => right.size - left.size),
    [items],
  );
  const totalBytes = useMemo(
    () => items.reduce((total, item) => total + item.size, 0),
    [items],
  );
  const percent =
    progress && progress.itemsTotal > 0
      ? Math.min(100, (progress.itemsDone / progress.itemsTotal) * 100)
      : 0;

  const startScan = async () => {
    if (scanInFlight.current) return;
    scanInFlight.current = true;

    setScanning(true);
    setHasScanned(false);
    setItems([]);
    setSelected(new Set());
    setSummary(null);
    setProgress(null);
    setReport(null);
    setError(null);

    const channel = new Channel<ScanEvent>();
    channel.onmessage = (event) => {
      switch (event.type) {
        case "started":
          setProgress({
            itemsDone: 0,
            itemsTotal: event.totalEntries,
            fileCount: 0,
            totalBytes: 0,
          });
          break;
        case "item":
          setItems((previous) =>
            previous.some((entry) => entry.id === event.item.id)
              ? previous
              : [...previous, event.item],
          );
          break;
        case "progress":
          setProgress({
            itemsDone: event.itemsDone,
            itemsTotal: event.itemsTotal,
            fileCount: event.fileCount,
            totalBytes: event.totalBytes,
          });
          break;
        case "finished":
          setProgress({
            itemsDone: event.itemCount,
            itemsTotal: Math.max(event.itemCount, 1),
            fileCount: event.fileCount,
            totalBytes: event.totalBytes,
          });
          break;
      }
    };

    try {
      const result = await invoke<ScanSummary>("scan_attention_areas", {
        onEvent: channel,
      });
      setSummary(result);
    } catch (scanError) {
      setError(toErrorMessage(scanError));
    } finally {
      scanInFlight.current = false;
      setScanning(false);
      setHasScanned(true);
    }
  };

  const toggleItem = useCallback((id: string) => {
    setSelected((previous) => {
      const next = new Set(previous);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  const confirmDelete = async ({ permanent }: DeleteConfirmOptions) => {
    if (selectedItems.length === 0 || deleteInFlight.current) return;
    deleteInFlight.current = true;
    setDeleting(true);

    try {
      const paths = selectedItems.map((item) => item.path);
      let result: DeleteReport;

      if (permanent) {
        const ticket = await invoke<PermanentTicket>(
          "prepare_permanent_delete",
          { paths },
        );
        result = await invoke<DeleteReport>("permanent_delete", {
          paths,
          token: ticket.token,
          allowAsk: true,
        });
      } else {
        result = await invoke<DeleteReport>("move_items_to_trash", {
          paths,
          allowAsk: true,
        });
      }

      const removed = new Set(result.removed.map((entry) => entry.path));
      setItems((previous) =>
        previous.filter((item) => !removed.has(item.path)),
      );
      setSelected((previous) => {
        const next = new Set(previous);
        for (const item of selectedItems) {
          if (removed.has(item.path)) next.delete(item.id);
        }
        return next;
      });
      setReport(result);
      setConfirmOpen(false);
    } catch (deleteError) {
      setError(toErrorMessage(deleteError));
      setConfirmOpen(false);
    } finally {
      deleteInFlight.current = false;
      setDeleting(false);
    }
  };

  return (
    <section className="panel attention-panel">
      <div className="panel-head">
        <div>
          <h2>
            需确认区域
            <span className="chip chip-ask">ask 档</span>
          </h2>
          <p>
            默认不勾选。这里的内容可能是设备数据、容器、模型或备份，删除前必须逐项确认。
          </p>
        </div>
        <button
          type="button"
          className="btn ghost"
          onClick={() => void startScan()}
          disabled={scanning}
        >
          {scanning ? <span className="spinner" aria-hidden /> : <ScanIcon />}
          {scanning ? "扫描中…" : hasScanned ? "重新扫描" : "扫描需确认区域"}
        </button>
      </div>

      {error && (
        <div className="notice is-error">
          <span className="notice-icon">
            <AlertIcon size={20} />
          </span>
          <div className="notice-copy">
            <strong>需确认区域扫描失败</strong>
            <p>{error}</p>
          </div>
          <button
            type="button"
            className="icon-btn"
            onClick={() => setError(null)}
            aria-label="关闭错误提示"
          >
            <CloseIcon />
          </button>
        </div>
      )}

      {report && (
        <div className="notice is-success">
          <span className="notice-icon">
            <CheckIcon size={20} />
          </span>
          <div className="notice-copy">
            <strong>
              {report.removed.length > 0
                ? report.mode === "permanent"
                  ? `已永久删除 ${formatBytes(report.freedBytes)}`
                  : `已移入废纸篓 ${formatBytes(report.freedBytes)}`
                : "没有项目被处理"}
            </strong>
            <p>
              {report.removed.length > 0
                ? report.mode === "permanent"
                  ? `${formatCount(report.removed.length)} 项已永久删除，无法恢复。`
                  : `${formatCount(report.removed.length)} 项已处理，可在废纸篓中「放回原处」恢复。`
                : "所选项目均被安全守卫跳过，未发生删除。"}
            </p>
            {report.skipped.length > 0 && (
              <details className="report-details">
                <summary>
                  {formatCount(report.skipped.length)} 项未处理，查看原因
                </summary>
                <ul>
                  {report.skipped.map((entry) => (
                    <li key={entry.path}>
                      <code>{entry.path}</code>
                      <span>{entry.reason}</span>
                    </li>
                  ))}
                </ul>
              </details>
            )}
          </div>
          <button
            type="button"
            className="icon-btn"
            onClick={() => setReport(null)}
            aria-label="关闭删除结果"
          >
            <CloseIcon />
          </button>
        </div>
      )}

      {scanning && (
        <div className="scan-progress">
          <div
            className={`progress-track${
              !progress || progress.itemsTotal === 0 ? " is-indeterminate" : ""
            }`}
          >
            <span
              style={
                progress && progress.itemsTotal > 0
                  ? { width: `${percent}%` }
                  : undefined
              }
            />
          </div>
          <span className="progress-label">
            {progress
              ? `${formatCount(progress.itemsDone)} / ${formatCount(
                  progress.itemsTotal,
                )}`
              : "准备中"}
          </span>
        </div>
      )}

      {items.length > 0 && (
        <ScanResultList
          items={sortedItems}
          selected={selected}
          onToggle={toggleItem}
          onToggleAll={() => setSelected(new Set())}
          allowSelectAll={false}
          toolbarHint="ask 档默认不勾选"
        />
      )}

      {items.length === 0 && (
        <div className="empty-state">
          <div className="empty-art" aria-hidden>
            <span />
            <span />
            <span />
          </div>
          <h3>
            {scanning
              ? "正在读取需确认区域"
              : hasScanned && error
                ? "扫描没有完成"
                : hasScanned
                  ? "没有发现需确认内容"
                  : "尚未扫描需确认区域"}
          </h3>
          <p>
            {scanning
              ? "只列出各区域的一级子项，根目录本身永远不会被删除。"
              : hasScanned
                ? "当前区域很干净，或相关目录尚未创建。"
                : "点击扫描，查看设备符号、容器、模型缓存与 iOS 备份的占用。"}
          </p>
        </div>
      )}

      {items.length > 0 && (
        <div className="actionbar attention-actionbar">
          <div className="selection">
            <span className="selection-mark" aria-hidden />
            <span>
              已选 <strong>{formatCount(selectedItems.length)}</strong> 项
            </span>
            <span className="selection-separator">·</span>
            <span className="selection-bytes">{formatBytes(selectedBytes)}</span>
            <span className="selection-separator">·</span>
            <span className="selection-bytes">
              共 {formatBytes(totalBytes)}
              {summary?.itemCount ? ` · ${formatCount(summary.itemCount)} 项` : ""}
            </span>
          </div>
          <div className="actionbar-right">
            <span className="actionbar-hint">ask 档必须逐项确认</span>
            <button
              type="button"
              className="btn primary"
              onClick={() => setConfirmOpen(true)}
              disabled={selectedItems.length === 0 || scanning}
            >
              <TrashIcon />
              清理所选
            </button>
          </div>
        </div>
      )}

      <DeleteConfirmDialog
        open={confirmOpen}
        count={selectedItems.length}
        totalBytes={selectedBytes}
        busy={deleting}
        title="确认清理需确认区域？"
        notes={NOTES}
        confirmLabel="清理所选"
        requireAcknowledgement
        acknowledgementLabel="我已逐项确认这些内容不再需要，并了解删除后可能无法恢复。"
        danger
        allowPermanent
        onCancel={() => setConfirmOpen(false)}
        onConfirm={(options) => void confirmDelete(options)}
      />
    </section>
  );
}
