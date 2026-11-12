import { useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  DeleteReport,
  DuplicateGroup,
  DuplicateScanOptions,
} from "../types";
import { useDuplicateScan } from "../hooks/useDuplicateScan";
import { DeleteConfirmDialog } from "../components/DeleteConfirmDialog";
import type { DeleteConfirmOptions } from "../components/DeleteConfirmDialog";
import { FdaNotice } from "../components/FdaNotice";
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
  formatDate,
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
  { label: "1 MB", value: 1 * MB },
  { label: "10 MB", value: 10 * MB },
  { label: "100 MB", value: 100 * MB },
];

const NOTES = [
  "每组至少保留一份，保留项已锁定，不能勾选删除。",
  "删除前后端会重新校验保留项存在且内容哈希一致。",
  "删除默认移入废纸篓，可在废纸篓中恢复。",
];

interface DuplicatesPageProps {
  fda: boolean | null;
  onRecheck: () => void;
  onOpenSettings: () => void;
}

export function DuplicatesPage({
  fda,
  onRecheck,
  onOpenSettings,
}: DuplicatesPageProps) {
  const scan = useDuplicateScan();
  const {
    status,
    groups,
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
  const [minSizeBytes, setMinSizeBytes] = useState(1 * MB);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [report, setReport] = useState<DeleteReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const deleteInFlight = useRef(false);

  const scanning = status === "scanning";
  const displayError = error ?? scanError;

  const selectedGroups = useMemo(() => {
    return groups
      .map((group) => {
        const keep = group.files.find((file) => file.isKeep);
        if (!keep) return null;
        const deletePaths = group.files
          .filter((file) => !file.isKeep && selected.has(file.id))
          .map((file) => file.path);
        if (deletePaths.length === 0) return null;
        return { id: group.id, keepPath: keep.path, deletePaths };
      })
      .filter((group): group is NonNullable<typeof group> => group !== null);
  }, [groups, selected]);

  const selectedFiles = useMemo(
    () =>
      groups.flatMap((group) =>
        group.files.filter((file) => !file.isKeep && selected.has(file.id)),
      ),
    [groups, selected],
  );
  const selectedBytes = useMemo(
    () => selectedFiles.reduce((total, file) => total + file.size, 0),
    [selectedFiles],
  );
  const totalGroups = groups.length;
  const totalFiles = useMemo(
    () => groups.reduce((total, group) => total + group.totalFiles, 0),
    [groups],
  );
  const reclaimable = summary?.reclaimableBytes ?? 0;
  const filesScanned = summary?.filesScanned ?? progress?.filesScanned ?? 0;
  const durationText = summary
    ? summary.durationMs < 1000
      ? `${summary.durationMs} ms`
      : `${(summary.durationMs / 1000).toFixed(1)} s`
    : scanning
      ? "…"
      : "—";

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
  };

  const toggleFile = (id: string) => {
    setSelected((previous) => {
      const next = new Set(previous);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const toggleGroup = (group: DuplicateGroup) => {
    const selectable = group.files.filter((file) => !file.isKeep);
    setSelected((previous) => {
      const next = new Set(previous);
      const allSelected = selectable.every((file) => next.has(file.id));
      for (const file of selectable) {
        if (allSelected) {
          next.delete(file.id);
        } else {
          next.add(file.id);
        }
      }
      return next;
    });
  };

  const toggleAll = () => {
    setSelected((previous) => {
      const allIds = groups.flatMap((group) =>
        group.files.filter((file) => !file.isKeep).map((file) => file.id),
      );
      const allSelected =
        allIds.length > 0 && allIds.every((id) => previous.has(id));
      return allSelected ? new Set() : new Set(allIds);
    });
  };

  const startScan = async () => {
    if (selectedRoots.size === 0) {
      setError("请至少选择一个扫描目录");
      return;
    }
    setSelected(new Set());
    setReport(null);
    setError(null);

    const options: DuplicateScanOptions = {
      roots: Array.from(selectedRoots),
      minSizeBytes,
      maxGroups: 200,
    };
    await start(options);
  };

  const confirmDelete = async (_options: DeleteConfirmOptions) => {
    if (selectedGroups.length === 0 || deleteInFlight.current) return;
    deleteInFlight.current = true;
    setDeleting(true);

    try {
      const result = await invoke<DeleteReport>("delete_duplicates", {
        groups: selectedGroups,
        allowAsk: false,
      });

      const removed = new Set(result.removed.map((entry) => entry.path));
      removePaths(removed);
      setSelected((previous) => {
        const next = new Set(previous);
        for (const path of removed) next.delete(path);
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
          <p className="eyebrow">DUPLICATES · 重复文件</p>
          <h1>相同内容，只保留一份。</h1>
          <p className="lede">
            大小分组、前 4 KB 哈希、完整 BLAKE3 三段式检测。
            每组至少保留一份，保留项不可勾选。
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
            {scanning ? "扫描中…" : summary ? "重新扫描" : "开始扫描"}
          </button>
          <p className="scope">
            <code>下载 / 桌面 / 文档 / 影片</code>
            <span>只读取文件内容做哈希，不上传任何数据</span>
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
                  ? `${formatCount(report.removed.length)} 个副本已永久删除，每组保留项仍然存在。`
                  : `${formatCount(report.removed.length)} 个副本已处理，可在废纸篓中恢复。`
                : "所选文件均被安全守卫跳过，未发生删除。"}
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

      <section className="scan-options" aria-label="重复文件扫描选项">
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
          <span className="option-label">最小文件大小</span>
          <div className="option-chips">
            {MIN_SIZE_OPTIONS.map((option) => (
              <button
                key={option.value}
                type="button"
                className={`option-chip is-compact${
                  minSizeBytes === option.value ? " is-active" : ""
                }`}
                onClick={() => setMinSizeBytes(option.value)}
                disabled={scanning}
                aria-pressed={minSizeBytes === option.value}
              >
                {option.label}
              </button>
            ))}
          </div>
          <span className="option-note">
            小于阈值的文件不参与检测，避免大量碎文件拖慢扫描。
          </span>
        </div>
      </section>

      <section className="metric-strip" aria-label="重复文件统计">
        <div className="metric hero-metric">
          <span className="metric-label">可回收副本空间</span>
          <strong>{formatBytes(reclaimable)}</strong>
        </div>
        <div className="metric">
          <span className="metric-label">重复分组</span>
          <strong>{formatCount(totalGroups)}</strong>
        </div>
        <div className="metric">
          <span className="metric-label">重复文件</span>
          <strong>{formatCount(totalFiles)}</strong>
        </div>
        <div className="metric">
          <span className="metric-label">已扫描 / 耗时</span>
          <strong>{formatCount(filesScanned)}</strong>
          <small>{durationText}</small>
        </div>
      </section>

      <section className="panel">
        <div className="panel-head">
          <div>
            <h2>
              重复分组
              {groups.length > 0 && (
                <span className="count-badge">{formatCount(groups.length)}</span>
              )}
            </h2>
            <p>
              {scanning
                ? "正在计算哈希，完成后按可回收空间从大到小展示。"
                : summary
                  ? `扫描完成 · 跳过 ${formatCount(summary.skippedEntries)} 个无权限或链接条目`
                  : "选择目录和最小大小后开始扫描。"}
            </p>
          </div>
          <span className="root-chip">
            {summary ? `${summary.roots.length} 个目录` : "用户目录"}
          </span>
        </div>

        {scanning && (
          <div className="scan-progress">
            <div className="progress-track is-indeterminate">
              <span />
            </div>
            <span className="progress-label">
              {formatCount(progress?.filesScanned ?? 0)} 个文件 ·{" "}
              {formatCount(progress?.filesHashed ?? 0)} 次哈希 ·{" "}
              {formatBytes(progress?.bytesHashed ?? 0)}
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

        {groups.length > 0 && (
          <div className="dup-layout">
            <div className="list-toolbar">
              <label className="checkline">
                <input
                  type="checkbox"
                  checked={
                    groups.length > 0 &&
                    groups.every((group) =>
                      group.files
                        .filter((file) => !file.isKeep)
                        .every((file) => selected.has(file.id)),
                    )
                  }
                  onChange={toggleAll}
                  aria-label="全选所有副本"
                />
                全选副本
              </label>
              <span className="list-count">
                {formatCount(groups.length)} 组 · 保留项已锁定
              </span>
            </div>

            <div className="dup-groups">
              {groups.map((group) => {
                const selectable = group.files.filter((file) => !file.isKeep);
                const groupSelected = selectable.filter((file) =>
                  selected.has(file.id),
                );
                return (
                  <div className="dup-group" key={group.id}>
                    <div className="dup-group-head">
                      <div>
                        <strong>{formatBytes(group.size)}</strong>
                        <span>
                          {formatCount(group.totalFiles)} 个副本 · 可回收{" "}
                          {formatBytes(group.wastedBytes)}
                        </span>
                      </div>
                      <button
                        type="button"
                        className="btn small ghost"
                        onClick={() => toggleGroup(group)}
                        disabled={scanning}
                      >
                        {groupSelected.length === selectable.length
                          ? "取消本组副本"
                          : "选择本组副本"}
                      </button>
                    </div>

                    <div className="dup-files">
                      {group.files.map((file) => {
                        const checked = !file.isKeep && selected.has(file.id);
                        return (
                          <label
                            className={`dup-file${
                              file.isKeep ? " is-keep" : ""
                            }${checked ? " is-selected" : ""}`}
                            key={file.id}
                          >
                            <span className="checkbox">
                              <input
                                type="checkbox"
                                checked={file.isKeep || checked}
                                disabled={file.isKeep}
                                onChange={() => toggleFile(file.id)}
                                aria-label={
                                  file.isKeep
                                    ? `保留 ${file.name}`
                                    : `删除副本 ${file.name}`
                                }
                              />
                              <span className="box" aria-hidden />
                            </span>
                            <span className="dup-main">
                              <span className="dup-name" title={file.name}>
                                {file.name}
                              </span>
                              <code className="dup-path" title={file.path}>
                                {file.path}
                              </code>
                            </span>
                            <span className="dup-date">
                              {formatDate(file.modifiedMs)}
                            </span>
                            <span className="dup-size">
                              {formatBytes(file.size)}
                            </span>
                            <span
                              className={`chip ${
                                file.isKeep ? "chip-safe" : "chip-neutral"
                              }`}
                            >
                              {file.isKeep ? "保留" : "副本"}
                            </span>
                          </label>
                        );
                      })}
                    </div>

                    {group.totalFiles > group.files.length && (
                      <p className="dup-truncated">
                        仅显示前 {group.files.length} 个，组内共{" "}
                        {formatCount(group.totalFiles)} 个文件。
                      </p>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {groups.length === 0 && (
          <div className="empty-state">
            <div className="empty-art" aria-hidden>
              <span />
              <span />
              <span />
            </div>
            <h3>
              {scanning
                ? "正在计算重复文件"
                : status === "done"
                  ? "没有发现重复文件"
                  : "还没有扫描结果"}
            </h3>
            <p>
              {scanning
                ? "先按大小分组，再比对前 4 KB 与完整内容；扫描完成前不会展示半成品分组。"
                : status === "done"
                  ? "可以调整目录或降低最小文件大小后重新扫描。"
                  : "选择扫描目录后开始，每组至少保留一份。"}
            </p>
            {!scanning && (
              <button
                type="button"
                className="btn ghost"
                onClick={() => void startScan()}
              >
                {summary ? "重新扫描" : "开始扫描"}
              </button>
            )}
          </div>
        )}
      </section>

      {groups.length > 0 && (
        <div className="actionbar">
          <div className="selection">
            <span className="selection-mark" aria-hidden />
            <span>
              已选 <strong>{formatCount(selectedFiles.length)}</strong> 个副本
            </span>
            <span className="selection-separator">·</span>
            <span className="selection-bytes">{formatBytes(selectedBytes)}</span>
          </div>
          <div className="actionbar-right">
            <span className="actionbar-hint">每组至少保留一份</span>
            <button
              type="button"
              className="btn primary"
              onClick={() => setConfirmOpen(true)}
              disabled={selectedFiles.length === 0 || scanning}
            >
              <TrashIcon />
              删除所选副本
            </button>
          </div>
        </div>
      )}

      <DeleteConfirmDialog
        open={confirmOpen}
        count={selectedFiles.length}
        totalBytes={selectedBytes}
        busy={deleting}
        title="确认删除重复副本？"
        notes={NOTES}
        confirmLabel="删除所选副本"
        onCancel={() => setConfirmOpen(false)}
        onConfirm={(options) => void confirmDelete(options)}
      />
    </div>
  );
}
