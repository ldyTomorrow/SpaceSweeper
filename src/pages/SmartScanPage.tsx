import { useCallback, useMemo, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  CacheItem,
  DeleteReport,
  PermanentTicket,
  ScanEvent,
  ScanSummary,
} from "../types";
import {
  displayHomePath,
  formatBytes,
  formatCount,
  toErrorMessage,
} from "../lib/format";
import { ScanResultList } from "../components/ScanResultList";
import { DeleteConfirmDialog } from "../components/DeleteConfirmDialog";
import type { DeleteConfirmOptions } from "../components/DeleteConfirmDialog";
import { AttentionPanel } from "../components/AttentionPanel";
import { FdaNotice } from "../components/FdaNotice";
import {
  AlertIcon,
  CheckIcon,
  CloseIcon,
  ScanIcon,
  TrashIcon,
} from "../components/Icons";

interface LiveProgress {
  itemsDone: number;
  itemsTotal: number;
  fileCount: number;
  totalBytes: number;
}

interface SmartScanPageProps {
  fda: boolean | null;
  onRecheck: () => void;
  onOpenSettings: () => void;
}

const CACHE_GROUPS = [
  { id: "app", label: "应用缓存", hint: "~/Library/Caches" },
  { id: "logs", label: "日志", hint: "~/Library/Logs" },
  { id: "developer", label: "开发者缓存", hint: "DerivedData / 模拟器" },
  { id: "packages", label: "包管理器缓存", hint: "npm / pip / Cargo / Gradle" },
];

export function SmartScanPage({
  fda,
  onRecheck,
  onOpenSettings,
}: SmartScanPageProps) {
  const [items, setItems] = useState<CacheItem[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [groups, setGroups] = useState<Set<string>>(new Set(["app"]));
  const [scanning, setScanning] = useState(false);
  const [hasScanned, setHasScanned] = useState(false);
  const [summary, setSummary] = useState<ScanSummary | null>(null);
  const [progress, setProgress] = useState<LiveProgress | null>(null);
  const [scanRoot, setScanRoot] = useState("~/Library/Caches");
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<DeleteReport | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const scanInFlight = useRef(false);
  const deleteInFlight = useRef(false);

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

  const toggleAll = useCallback(() => {
    setSelected((previous) => {
      if (items.length > 0 && previous.size === items.length) return new Set();
      return new Set(items.map((item) => item.id));
    });
  }, [items]);

  const toggleGroup = useCallback((id: string) => {
    setGroups((previous) => {
      const next = new Set(previous);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  const closeConfirm = useCallback(() => setConfirmOpen(false), []);

  const selectedItems = useMemo(
    () => items.filter((item) => selected.has(item.id)),
    [items, selected],
  );
  const sortedItems = useMemo(
    () => [...items].sort((left, right) => right.size - left.size),
    [items],
  );
  const selectedBytes = useMemo(
    () => selectedItems.reduce((total, item) => total + item.size, 0),
    [selectedItems],
  );

  const startScan = async () => {
    if (scanInFlight.current) return;
    if (groups.size === 0) {
      setError("请至少选择一个缓存分组");
      return;
    }
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
          setScanRoot(event.root);
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
          // safe 档默认勾选；用户仍可逐项取消。ask 档必须由用户显式勾选。
          if (event.item.safetyTier === "safe") {
            setSelected((previous) => {
              if (previous.has(event.item.id)) return previous;
              const next = new Set(previous);
              next.add(event.item.id);
              return next;
            });
          }
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
      const result = await invoke<ScanSummary>("scan_library_caches", {
        groups: Array.from(groups),
        onEvent: channel,
      });
      setSummary(result);
      setScanRoot(result.root);
    } catch (scanError) {
      setError(toErrorMessage(scanError));
    } finally {
      scanInFlight.current = false;
      setScanning(false);
      setHasScanned(true);
    }
  };

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
          allowAsk: false,
        });
      } else {
        result = await invoke<DeleteReport>("move_items_to_trash", {
          paths,
          allowAsk: false,
        });
      }

      const removedPaths = new Set(result.removed.map((entry) => entry.path));
      setItems((previous) =>
        previous.filter((item) => !removedPaths.has(item.path)),
      );
      setSelected((previous) => {
        const next = new Set(previous);
        for (const item of selectedItems) {
          if (removedPaths.has(item.path)) next.delete(item.id);
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

  const totalBytes = useMemo(
    () => items.reduce((total, item) => total + item.size, 0),
    [items],
  );
  const totalItems = items.length;
  const totalFiles = useMemo(
    () => items.reduce((total, item) => total + item.fileCount, 0),
    [items],
  );
  const percent =
    progress && progress.itemsTotal > 0
      ? Math.min(100, (progress.itemsDone / progress.itemsTotal) * 100)
      : 0;
  const durationText = summary
    ? summary.durationMs < 1000
      ? `${summary.durationMs} ms`
      : `${(summary.durationMs / 1000).toFixed(1)} s`
    : scanning
      ? "…"
      : "—";

  return (
    <div className="page">
      <section className="hero">
        <div className="hero-copy">
          <p className="eyebrow">SMART SCAN · 智能扫描</p>
          <h1>先看清，再清理。</h1>
          <p className="lede">
            只扫描可再生的应用缓存，每条结果都给出完整路径、大小与判定理由。
          </p>
        </div>

        <div className="hero-actions">
          <button
            type="button"
            className="btn primary scan-btn"
            onClick={() => void startScan()}
            disabled={scanning}
          >
            {scanning ? <span className="spinner" aria-hidden /> : <ScanIcon />}
            {scanning ? "扫描中…" : hasScanned ? "重新扫描" : "开始扫描"}
          </button>
          <p className="scope">
            <code>{displayHomePath(scanRoot)}</code>
            <span>仅缓存，不碰个人数据与系统文件</span>
          </p>
        </div>
      </section>

      {fda === false && (
        <FdaNotice onRecheck={onRecheck} onOpenSettings={onOpenSettings} />
      )}

      {error && (
        <div className="notice is-error">
          <span className="notice-icon">
            <AlertIcon size={20} />
          </span>
          <div className="notice-copy">
            <strong>操作没有完成</strong>
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

      <section className="scan-options" aria-label="缓存扫描分组">
        <div className="option-group">
          <span className="option-label">缓存分组</span>
          <div className="option-chips">
            {CACHE_GROUPS.map((group) => (
              <button
                key={group.id}
                type="button"
                className={`option-chip${
                  groups.has(group.id) ? " is-active" : ""
                }`}
                onClick={() => toggleGroup(group.id)}
                disabled={scanning}
                aria-pressed={groups.has(group.id)}
              >
                <span>{group.label}</span>
                <small>{group.hint}</small>
              </button>
            ))}
          </div>
          <span className="option-note">
            只扫描可再生缓存与日志；需确认区域在下方单独扫描。
          </span>
        </div>
      </section>

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
                  : `${formatCount(report.removed.length)} 项已处理，可在废纸篓中「放回原处」恢复；清空废纸篓后释放空间。`
                : "所选项目均被安全守卫跳过，未发生删除。"}
            </p>
            {report.removed.length > 0 && (
              <details className="report-details">
                <summary>
                  {report.mode === "permanent"
                    ? "查看已永久删除的完整路径"
                    : "查看已移入废纸篓的完整路径"}
                </summary>
                <ul>
                  {report.removed.map((entry) => (
                    <li key={entry.path}>
                      <code>{entry.path}</code>
                      <span>{formatBytes(entry.size)}</span>
                    </li>
                  ))}
                </ul>
              </details>
            )}
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

      <section className="metric-strip" aria-label="扫描统计">
        <div className="metric hero-metric">
          <span className="metric-label">可回收空间</span>
          <strong>{formatBytes(totalBytes)}</strong>
        </div>
        <div className="metric">
          <span className="metric-label">扫描条目</span>
          <strong>{formatCount(totalItems)}</strong>
        </div>
        <div className="metric">
          <span className="metric-label">涉及文件</span>
          <strong>{formatCount(totalFiles)}</strong>
        </div>
        <div className="metric">
          <span className="metric-label">扫描耗时</span>
          <strong>{durationText}</strong>
        </div>
      </section>

      <section className="panel">
        <div className="panel-head">
          <div>
            <h2>
              扫描结果
              {items.length > 0 && (
                <span className="count-badge">{formatCount(items.length)}</span>
              )}
            </h2>
            <p>
              {scanning
                ? "结果正在实时出现，可以先浏览已发现的条目。"
                : summary
                  ? `扫描完成 · 跳过 ${formatCount(
                      summary.skippedEntries + summary.deniedEntries,
                    )} 个无权限或符号链接条目`
                  : "safe 档默认勾选；ask 档不会出现在本页。"}
            </p>
          </div>
          <span className="root-chip" title={scanRoot}>
            {displayHomePath(scanRoot)}
          </span>
        </div>

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
            onToggleAll={toggleAll}
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
                ? "正在读取缓存目录"
                : hasScanned && error
                  ? "扫描没有完成"
                  : hasScanned
                    ? "没有发现可回收的缓存"
                    : "还没有扫描结果"}
            </h3>
            <p>
              {scanning
                ? "条目会在计算完成后逐个出现，不需要等待全部扫描结束。"
                : hasScanned && error
                  ? "请查看上方的错误提示，修复后重新扫描。"
                  : hasScanned
                    ? "这个目录目前很干净。可以稍后重新扫描，或等待应用产生新的缓存。"
                    : "点击「开始扫描」，结果会实时出现在这里。所有删除默认进入废纸篓。"}
            </p>
            {!scanning && (
              <button
                type="button"
                className="btn ghost"
                onClick={() => void startScan()}
              >
                {hasScanned ? "再次扫描" : "开始扫描"}
              </button>
            )}
          </div>
        )}
      </section>

      {items.length > 0 && (
        <div className="actionbar">
          <div className="selection">
            <span className="selection-mark" aria-hidden />
            <span>
              已选 <strong>{formatCount(selectedItems.length)}</strong> 项
            </span>
            <span className="selection-separator">·</span>
            <span className="selection-bytes">
              {formatBytes(selectedBytes)}
            </span>
          </div>
          <div className="actionbar-right">
            <span className="actionbar-hint">默认移入废纸篓；可在确认框中选择永久删除</span>
            <button
              type="button"
              className="btn primary"
              onClick={() => setConfirmOpen(true)}
              disabled={selectedItems.length === 0 || scanning}
            >
              <TrashIcon />
              移入废纸篓
            </button>
          </div>
        </div>
      )}

      <AttentionPanel />

      <DeleteConfirmDialog
        open={confirmOpen}
        count={selectedItems.length}
        totalBytes={selectedBytes}
        busy={deleting}
        onCancel={closeConfirm}
        onConfirm={(options) => void confirmDelete(options)}
        allowPermanent
      />
    </div>
  );
}
