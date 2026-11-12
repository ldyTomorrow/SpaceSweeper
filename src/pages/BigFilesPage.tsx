import { useCallback, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DeleteReport, LargeFileItem, PermanentTicket } from "../types";
import { useLargeFilesScan } from "../hooks/useLargeFilesScan";
import { DeleteConfirmDialog } from "../components/DeleteConfirmDialog";
import type { DeleteConfirmOptions } from "../components/DeleteConfirmDialog";
import { FdaNotice } from "../components/FdaNotice";
import { LargeFileList } from "../components/LargeFileList";
import {
  AlertIcon,
  CheckIcon,
  CloseIcon,
  ScanIcon,
  TrashIcon,
} from "../components/Icons";
import {
  formatBytes,
  formatCount,
  toErrorMessage,
} from "../lib/format";

const MB = 1024 * 1024;

const ROOT_OPTIONS = [
  { name: "Downloads", label: "下载", path: "~/Downloads" },
  { name: "Desktop", label: "桌面", path: "~/Desktop" },
  { name: "Documents", label: "文档", path: "~/Documents" },
  { name: "Movies", label: "影片", path: "~/Movies" },
];

const MIN_SIZE_OPTIONS = [
  { label: "100 MB", value: 100 * MB },
  { label: "500 MB", value: 500 * MB },
  { label: "1 GB", value: 1024 * MB },
];

const OLD_AGE_OPTIONS = [
  { label: "1 年", value: 365 },
  { label: "2 年", value: 730 },
  { label: "3 年", value: 1095 },
];

type TabKey = "big" | "old" | "installers";

const TABS: { key: TabKey; label: string }[] = [
  { key: "big", label: "大文件" },
  { key: "old", label: "旧文件" },
  { key: "installers", label: "旧安装包" },
];

const DELETE_NOTES = [
  "仅删除你勾选的文件，不会删除文件夹或未选中的内容。",
  "删除后仍可在废纸篓中「放回原处」恢复；清空废纸篓后才会释放空间。",
  "版本库、隐藏目录与系统文件不会出现在扫描结果中。",
];

interface BigFilesPageProps {
  fda: boolean | null;
  onRecheck: () => void;
  onOpenSettings: () => void;
}

export function BigFilesPage({
  fda,
  onRecheck,
  onOpenSettings,
}: BigFilesPageProps) {
  const scan = useLargeFilesScan();
  const {
    status,
    big,
    old,
    installers,
    summary,
    progress,
    error: scanError,
    start,
    cancel,
    removePaths,
    clearError,
  } = scan;

  const [selectedRoots, setSelectedRoots] = useState<Set<string>>(
    new Set(ROOT_OPTIONS.map((root) => root.name)),
  );
  const [minSizeBytes, setMinSizeBytes] = useState(100 * MB);
  const [oldDays, setOldDays] = useState(365);
  const [activeTab, setActiveTab] = useState<TabKey>("big");
  const [selectedByTab, setSelectedByTab] = useState<
    Record<TabKey, Set<string>>
  >({
    big: new Set(),
    old: new Set(),
    installers: new Set(),
  });
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [report, setReport] = useState<DeleteReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const deleteInFlight = useRef(false);

  const scanning = status === "scanning";
  const lists: Record<TabKey, LargeFileItem[]> = { big, old, installers };
  const currentItems = lists[activeTab];
  const currentSelectedSet = selectedByTab[activeTab];
  const currentSelected = useMemo(
    () => currentItems.filter((item) => currentSelectedSet.has(item.id)),
    [currentItems, currentSelectedSet],
  );
  const currentBytes = useMemo(
    () => currentItems.reduce((total, item) => total + item.size, 0),
    [currentItems],
  );
  const selectedBytes = useMemo(
    () => currentSelected.reduce((total, item) => total + item.size, 0),
    [currentSelected],
  );
  const filesScanned = summary?.filesScanned ?? progress?.filesScanned ?? 0;
  const durationText = summary
    ? summary.durationMs < 1000
      ? `${summary.durationMs} ms`
      : `${(summary.durationMs / 1000).toFixed(1)} s`
    : scanning
      ? "…"
      : "—";
  const displayError = error ?? scanError;
  const hasResults = summary !== null;

  const markDirty = useCallback(() => {
    if (hasResults) setDirty(true);
  }, [hasResults]);

  const closeConfirm = useCallback(() => setConfirmOpen(false), []);

  const startScan = async () => {
    const roots = Array.from(selectedRoots);
    if (roots.length === 0) {
      setError("请至少选择一个扫描目录");
      return;
    }

    setSelectedByTab({
      big: new Set(),
      old: new Set(),
      installers: new Set(),
    });
    setReport(null);
    setError(null);
    setDirty(false);

    await start({
      roots,
      minSizeBytes,
      oldDays,
      installerDays: 180,
      topN: 50,
    });
  };

  const toggleItem = useCallback(
    (id: string) => {
      setSelectedByTab((previous) => {
        const nextSet = new Set(previous[activeTab]);
        if (nextSet.has(id)) {
          nextSet.delete(id);
        } else {
          nextSet.add(id);
        }
        return { ...previous, [activeTab]: nextSet };
      });
    },
    [activeTab],
  );

  const toggleAll = useCallback(() => {
    setSelectedByTab((previous) => {
      const current = previous[activeTab];
      const shouldClear =
        currentItems.length > 0 && current.size === currentItems.length;
      const nextSet = shouldClear
        ? new Set<string>()
        : new Set(currentItems.map((item) => item.id));
      return { ...previous, [activeTab]: nextSet };
    });
  }, [activeTab, currentItems]);

  const toggleRoot = (name: string) => {
    setSelectedRoots((previous) => {
      const next = new Set(previous);
      if (next.has(name)) {
        next.delete(name);
      } else {
        next.add(name);
      }
      return next;
    });
    markDirty();
  };

  const selectMinSize = (value: number) => {
    setMinSizeBytes(value);
    markDirty();
  };

  const selectOldDays = (value: number) => {
    setOldDays(value);
    markDirty();
  };

  const revealInFinder = async (path: string) => {
    try {
      await invoke("reveal_in_finder", { path });
    } catch (revealError) {
      setError(toErrorMessage(revealError));
    }
  };

  const confirmDelete = async ({ permanent }: DeleteConfirmOptions) => {
    if (currentSelected.length === 0 || deleteInFlight.current) return;
    deleteInFlight.current = true;
    setDeleting(true);

    try {
      const paths = currentSelected.map((item) => item.path);
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

      const removed = new Set(result.removed.map((entry) => entry.path));
      removePaths(removed);
      setSelectedByTab((previous) => {
        const next: Record<TabKey, Set<string>> = {
          big: new Set(previous.big),
          old: new Set(previous.old),
          installers: new Set(previous.installers),
        };
        for (const paths of Object.values(next)) {
          for (const path of removed) paths.delete(path);
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

  const dismissError = () => {
    setError(null);
    clearError();
  };

  return (
    <div className="page">
      <section className="hero">
        <div className="hero-copy">
          <p className="eyebrow">LARGE FILES · 大文件</p>
          <h1>把遗忘的空间找回来。</h1>
          <p className="lede">
            扫描下载、桌面、文档和影片目录，只读取文件元数据。
            版本库、隐藏目录与系统文件不会被发现，也不会被删除。
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
            {scanning ? "扫描中…" : hasResults ? "重新扫描" : "开始扫描"}
          </button>
          <p className="scope">
            <code>下载 / 桌面 / 文档 / 影片</code>
            <span>只读元数据 · 默认移入废纸篓</span>
          </p>
        </div>
      </section>

      {fda === false && (
        <FdaNotice onRecheck={onRecheck} onOpenSettings={onOpenSettings} />
      )}

      {displayError && (
        <div className="notice is-error">
          <span className="notice-icon">
            <AlertIcon size={20} />
          </span>
          <div className="notice-copy">
            <strong>操作没有完成</strong>
            <p>{displayError}</p>
          </div>
          <button
            type="button"
            className="icon-btn"
            onClick={dismissError}
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
                : "没有文件被处理"}
            </strong>
            <p>
              {report.removed.length > 0
                ? report.mode === "permanent"
                  ? `${formatCount(report.removed.length)} 项已永久删除，无法恢复。`
                  : `${formatCount(report.removed.length)} 项已处理，可在废纸篓中「放回原处」恢复；清空废纸篓后释放空间。`
                : "所选文件均被安全守卫跳过，未发生删除。"}
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

      {summary?.cancelled && status === "done" && (
        <div className="notice">
          <span className="notice-icon">
            <AlertIcon size={20} />
          </span>
          <div className="notice-copy">
            <strong>扫描已取消</strong>
            <p>下面展示的是取消前已经发现的部分结果，可以继续查看或清理。</p>
          </div>
        </div>
      )}

      <section className="scan-options" aria-label="扫描选项">
        <div className="option-group">
          <span className="option-label">扫描目录</span>
          <div className="option-chips">
            {ROOT_OPTIONS.map((root) => (
              <button
                key={root.name}
                type="button"
                className={`option-chip${
                  selectedRoots.has(root.name) ? " is-active" : ""
                }`}
                onClick={() => toggleRoot(root.name)}
                disabled={scanning}
                aria-pressed={selectedRoots.has(root.name)}
              >
                <span>{root.label}</span>
                <small>{root.path}</small>
              </button>
            ))}
          </div>
        </div>

        <div className="option-group">
          <span className="option-label">最低大小</span>
          <div className="option-chips">
            {MIN_SIZE_OPTIONS.map((option) => (
              <button
                key={option.value}
                type="button"
                className={`option-chip is-compact${
                  minSizeBytes === option.value ? " is-active" : ""
                }`}
                onClick={() => selectMinSize(option.value)}
                disabled={scanning}
                aria-pressed={minSizeBytes === option.value}
              >
                {option.label}
              </button>
            ))}
          </div>
        </div>

        <div className="option-group">
          <span className="option-label">旧文件阈值</span>
          <div className="option-chips">
            {OLD_AGE_OPTIONS.map((option) => (
              <button
                key={option.value}
                type="button"
                className={`option-chip is-compact${
                  oldDays === option.value ? " is-active" : ""
                }`}
                onClick={() => selectOldDays(option.value)}
                disabled={scanning}
                aria-pressed={oldDays === option.value}
              >
                {option.label}
              </button>
            ))}
          </div>
          <span className="option-note">旧安装包固定 ≥50 MB、≥180 天</span>
        </div>
      </section>

      {dirty && !scanning && (
        <p className="dirty-hint">筛选条件已更改，点击「重新扫描」后生效。</p>
      )}

      <section className="metric-strip" aria-label="扫描统计">
        <div className="metric hero-metric">
          <span className="metric-label">当前分组空间</span>
          <strong>{formatBytes(currentBytes)}</strong>
        </div>
        <div className="metric">
          <span className="metric-label">条目</span>
          <strong>{formatCount(currentItems.length)}</strong>
        </div>
        <div className="metric">
          <span className="metric-label">已扫描文件</span>
          <strong>{formatCount(filesScanned)}</strong>
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
              {currentItems.length > 0 && (
                <span className="count-badge">
                  {formatCount(currentItems.length)}
                </span>
              )}
            </h2>
            <p>
              {scanning
                ? "正在读取文件元数据，结果会在扫描完成后按分组展示。"
                : summary
                  ? `扫描完成 · ${summary.roots.length} 个目录 · 跳过 ${formatCount(
                      summary.skippedEntries,
                    )} 个无权限或链接条目`
                  : "选择目录和筛选条件后开始扫描。"}
            </p>
          </div>
          <span className="root-chip">
            {summary ? `${summary.roots.length} 个目录` : "用户目录"}
          </span>
        </div>

        <div className="result-tabs" role="tablist" aria-label="大文件分组">
          {TABS.map((tab) => (
            <button
              key={tab.key}
              type="button"
              role="tab"
              aria-selected={activeTab === tab.key}
              className={`result-tab${activeTab === tab.key ? " is-active" : ""}`}
              onClick={() => setActiveTab(tab.key)}
            >
              {tab.label}
              <span className="tab-count">{formatCount(lists[tab.key].length)}</span>
            </button>
          ))}
        </div>

        {scanning && (
          <div className="scan-progress">
            <div className="progress-track is-indeterminate">
              <span />
            </div>
            <span className="progress-label">
              {formatCount(filesScanned)} 个文件 ·{" "}
              {formatBytes(progress?.bytesScanned ?? 0)}
            </span>
            <button
              type="button"
              className="btn small ghost"
              onClick={() => void cancel()}
            >
              取消
            </button>
          </div>
        )}

        {currentItems.length > 0 && (
          <LargeFileList
            items={currentItems}
            selected={currentSelectedSet}
            onToggle={toggleItem}
            onToggleAll={toggleAll}
            onReveal={(path) => void revealInFinder(path)}
          />
        )}

        {currentItems.length === 0 && (
          <div className="empty-state">
            <div className="empty-art" aria-hidden>
              <span />
              <span />
              <span />
            </div>
            <h3>
              {scanning
                ? "正在扫描用户目录"
                : status === "done"
                  ? "当前分组没有符合条件的文件"
                  : "还没有扫描结果"}
            </h3>
            <p>
              {scanning
                ? "扫描完成前可以先查看已完成的部分；大文件扫描不会读取文件内容。"
                : status === "done"
                  ? "可以切换其他分组，或调整最低大小与旧文件阈值后重新扫描。"
                  : "选择扫描目录后开始扫描，结果会按大文件、旧文件和旧安装包分组展示。"}
            </p>
            {!scanning && (
              <button
                type="button"
                className="btn ghost"
                onClick={() => void startScan()}
              >
                {hasResults ? "重新扫描" : "开始扫描"}
              </button>
            )}
          </div>
        )}
      </section>

      {currentItems.length > 0 && (
        <div className="actionbar">
          <div className="selection">
            <span className="selection-mark" aria-hidden />
            <span>
              已选 <strong>{formatCount(currentSelected.length)}</strong> 项
            </span>
            <span className="selection-separator">·</span>
            <span className="selection-bytes">{formatBytes(selectedBytes)}</span>
          </div>
          <div className="actionbar-right">
            <span className="actionbar-hint">仅删除文件；可在确认框中选择永久删除</span>
            <button
              type="button"
              className="btn primary"
              onClick={() => setConfirmOpen(true)}
              disabled={currentSelected.length === 0 || scanning}
            >
              <TrashIcon />
              移入废纸篓
            </button>
          </div>
        </div>
      )}

      <DeleteConfirmDialog
        open={confirmOpen}
        count={currentSelected.length}
        totalBytes={selectedBytes}
        busy={deleting}
        onCancel={closeConfirm}
        onConfirm={(options) => void confirmDelete(options)}
        notes={DELETE_NOTES}
        allowPermanent
      />
    </div>
  );
}
