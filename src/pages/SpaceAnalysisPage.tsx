import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  DeleteReport,
  PermanentTicket,
  SpaceNode,
} from "../types";
import { useSpaceAnalysis } from "../hooks/useSpaceAnalysis";
import { DeleteConfirmDialog } from "../components/DeleteConfirmDialog";
import type { DeleteConfirmOptions } from "../components/DeleteConfirmDialog";
import { FdaNotice } from "../components/FdaNotice";
import { SpaceTreemap } from "../components/SpaceTreemap";
import {
  AlertIcon,
  CheckIcon,
  ChevronIcon,
  CloseIcon,
  FolderIcon,
  RevealIcon,
  ScanIcon,
  TrashIcon,
} from "../components/Icons";
import {
  displayHomePath,
  formatBytes,
  formatCount,
  formatDate,
  toErrorMessage,
} from "../lib/format";

interface SpaceAnalysisPageProps {
  fda: boolean | null;
  onRecheck: () => void;
  onOpenSettings: () => void;
}

function findNode(node: SpaceNode, path: string): SpaceNode | null {
  if (node.path === path) return node;
  for (const child of node.children) {
    const hit = findNode(child, path);
    if (hit) return hit;
  }
  return null;
}

function findStack(node: SpaceNode, path: string): SpaceNode[] {
  if (node.path === path) return [node];
  for (const child of node.children) {
    const stack = findStack(child, path);
    if (stack.length > 0) return [node, ...stack];
  }
  return [];
}

export function SpaceAnalysisPage({
  fda,
  onRecheck,
  onOpenSettings,
}: SpaceAnalysisPageProps) {
  const analysis = useSpaceAnalysis();
  const {
    status,
    tree,
    summary,
    progress,
    error: scanError,
    notice,
    home,
    start,
    cancel,
    chooseAndAnalyze,
    removePaths,
    clearError,
    clearNotice,
  } = analysis;

  const [currentPath, setCurrentPath] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [report, setReport] = useState<DeleteReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const deleteInFlight = useRef(false);

  const scanning = status === "scanning";
  const currentNode = useMemo(() => {
    if (!tree) return null;
    return findNode(tree, currentPath ?? tree.path) ?? tree;
  }, [tree, currentPath]);
  const stack = useMemo(() => {
    if (!tree || !currentNode) return [];
    return findStack(tree, currentNode.path);
  }, [tree, currentNode]);

  useEffect(() => {
    if (!tree) return;
    if (!currentPath || !findNode(tree, currentPath)) {
      setCurrentPath(tree.path);
    }
  }, [tree, currentPath]);

  useEffect(() => {
    setSelected(new Set());
  }, [currentPath]);

  const children = currentNode?.children ?? [];
  const selectedNodes = useMemo(
    () => children.filter((child) => selected.has(child.id)),
    [children, selected],
  );
  const selectedBytes = useMemo(
    () => selectedNodes.reduce((total, node) => total + node.size, 0),
    [selectedNodes],
  );
  const hasAsk = selectedNodes.some((node) => node.safetyTier === "ask");
  const displayError = error ?? scanError;
  const durationText = summary
    ? summary.durationMs < 1000
      ? `${summary.durationMs} ms`
      : `${(summary.durationMs / 1000).toFixed(1)} s`
    : scanning
      ? "…"
      : "—";

  const rootOptions = useMemo(() => {
    if (!home) return [];
    return [
      { label: "主目录", path: home },
      { label: "下载", path: `${home}/Downloads` },
      { label: "桌面", path: `${home}/Desktop` },
      { label: "文档", path: `${home}/Documents` },
      { label: "影片", path: `${home}/Movies` },
    ];
  }, [home]);

  const analyzeRoot = async (path: string) => {
    setSelected(new Set());
    setReport(null);
    setError(null);
    setCurrentPath(null);
    await start(path);
  };

  const openDir = (node: SpaceNode) => {
    setCurrentPath(node.path);
  };

  const rescanDir = async (node: SpaceNode) => {
    setSelected(new Set());
    setReport(null);
    setError(null);
    setCurrentPath(node.path);
    await start(node.path);
  };

  const toggleFile = (node: SpaceNode) => {
    if (!node.deletable || node.isDir) return;
    setSelected((previous) => {
      const next = new Set(previous);
      if (next.has(node.id)) {
        next.delete(node.id);
      } else {
        next.add(node.id);
      }
      return next;
    });
  };

  const reveal = async (path: string) => {
    try {
      await invoke("reveal_in_finder", { path });
    } catch (revealError) {
      setError(toErrorMessage(revealError));
    }
  };

  const confirmDelete = async ({ permanent }: DeleteConfirmOptions) => {
    if (selectedNodes.length === 0 || deleteInFlight.current) return;
    deleteInFlight.current = true;
    setDeleting(true);

    try {
      const paths = selectedNodes.map((node) => node.path);
      let result: DeleteReport;

      if (permanent) {
        const ticket = await invoke<PermanentTicket>(
          "prepare_permanent_delete",
          { paths },
        );
        result = await invoke<DeleteReport>("permanent_delete", {
          paths,
          token: ticket.token,
          allowAsk: hasAsk,
        });
      } else {
        result = await invoke<DeleteReport>("move_items_to_trash", {
          paths,
          allowAsk: hasAsk,
        });
      }

      const removed = new Set(result.removed.map((entry) => entry.path));
      removePaths(removed);
      setSelected(new Set());
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
          <p className="eyebrow">SPACE LENS · 空间分析</p>
          <h1>从树图看见空间去了哪里。</h1>
          <p className="lede">
            按目录聚合大小，逐层下钻。目录节点只能定位，文件节点通过白名单后才可删除。
          </p>
        </div>

        <div className="hero-actions">
          <button
            type="button"
            className="btn primary scan-btn"
            onClick={() => void start()}
            disabled={scanning}
          >
            {scanning ? <span className="spinner" aria-hidden /> : <ScanIcon />}
            {scanning ? "分析中…" : tree ? "重新分析主目录" : "分析主目录"}
          </button>
          <button
            type="button"
            className="btn ghost"
            onClick={() => void chooseAndAnalyze()}
            disabled={scanning}
          >
            <FolderIcon size={16} />
            选择目录或外接卷
          </button>
          <p className="scope">
            <code>{currentNode ? displayHomePath(currentNode.path) : "主目录"}</code>
            <span>只读分析 · 目录不可删除 · 删除默认进废纸篓</span>
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

      {notice && (
        <div className="notice">
          <span className="notice-icon">
            <AlertIcon size={20} />
          </span>
          <div className="notice-copy">
            <strong>目录授权</strong>
            <p>{notice}</p>
          </div>
          <button
            type="button"
            className="icon-btn"
            onClick={clearNotice}
            aria-label="关闭提示"
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
                  ? `${formatCount(report.removed.length)} 个文件已永久删除，无法恢复。`
                  : `${formatCount(report.removed.length)} 个文件已处理，可在废纸篓中恢复。`
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

      <section className="scan-options" aria-label="空间分析根目录">
        <div className="option-group">
          <span className="option-label">快速选择</span>
          <div className="option-chips">
            {rootOptions.map((option) => (
              <button
                key={option.path}
                type="button"
                className={`option-chip is-compact${
                  currentNode?.path === option.path ? " is-active" : ""
                }`}
                onClick={() => void analyzeRoot(option.path)}
                disabled={scanning}
              >
                {option.label}
              </button>
            ))}
          </div>
          <span className="option-note">
            选择自定义目录或外接卷后，仅对本次会话授权删除；重启后需要重新授权。
          </span>
        </div>
      </section>

      {scanning && (
        <div className="scan-progress">
          <div className="progress-track is-indeterminate">
            <span />
          </div>
          <span className="progress-label">
            {formatCount(progress?.filesScanned ?? 0)} 个文件 ·{" "}
            {formatBytes(progress?.bytesScanned ?? 0)} · 跳过{" "}
            {formatCount(progress?.skippedEntries ?? 0)}
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

      <section className="metric-strip" aria-label="空间分析统计">
        <div className="metric hero-metric">
          <span className="metric-label">当前目录占用</span>
          <strong>{formatBytes(currentNode?.size ?? 0)}</strong>
        </div>
        <div className="metric">
          <span className="metric-label">文件数</span>
          <strong>{formatCount(currentNode?.fileCount ?? 0)}</strong>
        </div>
        <div className="metric">
          <span className="metric-label">已扫描文件</span>
          <strong>{formatCount(summary?.filesScanned ?? 0)}</strong>
        </div>
        <div className="metric">
          <span className="metric-label">耗时</span>
          <strong>{durationText}</strong>
        </div>
      </section>

      <section className="panel space-panel">
        <div className="panel-head">
          <div>
            <h2>
              {currentNode?.name ?? "空间树图"}
              {currentNode && (
                <span className="count-badge">{formatBytes(currentNode.size)}</span>
              )}
            </h2>
            <p>
              {currentNode
                ? `${formatCount(currentNode.fileCount)} 个文件 · 点击目录下钻，点击文件勾选`
                : "选择目录后开始分析。"}
            </p>
          </div>
          {currentNode && (
            <div className="space-head-actions">
              <button
                type="button"
                className="btn small ghost"
                onClick={() => void reveal(currentNode.path)}
              >
                <RevealIcon />
                定位
              </button>
              <button
                type="button"
                className="btn small ghost"
                onClick={() => void rescanDir(currentNode)}
                disabled={scanning}
              >
                重新分析此目录
              </button>
            </div>
          )}
        </div>

        {stack.length > 1 && (
          <div className="breadcrumb" aria-label="目录面包屑">
            {stack.map((node, index) => (
              <span key={node.path}>
                {index > 0 && <ChevronIcon className="breadcrumb-sep" />}
                <button
                  type="button"
                  className="breadcrumb-item"
                  onClick={() => setCurrentPath(node.path)}
                >
                  {node.name}
                </button>
              </span>
            ))}
          </div>
        )}

        {currentNode && currentNode.children.length > 0 && (
          <SpaceTreemap
            node={currentNode}
            selected={selected}
            onOpenDir={openDir}
            onRescanDir={(node) => void rescanDir(node)}
            onToggleFile={toggleFile}
          />
        )}

        {currentNode && children.length > 0 && (
          <div className="space-children">
            <div className="space-child-head" aria-hidden>
              <span />
              <span>名称 / 路径</span>
              <span className="align-right">大小</span>
              <span>修改时间</span>
              <span>操作</span>
            </div>
            {children.map((child) => (
              <div className="space-child" key={child.id}>
                <span className="space-child-check">
                  {child.deletable && !child.isDir ? (
                    <label className="checkbox">
                      <input
                        type="checkbox"
                        checked={selected.has(child.id)}
                        onChange={() => toggleFile(child)}
                        aria-label={`选择 ${child.name}`}
                      />
                      <span className="box" aria-hidden />
                    </label>
                  ) : (
                    <span className="space-lock" aria-hidden>
                      {child.isDir ? <FolderIcon size={15} /> : "—"}
                    </span>
                  )}
                </span>

                <div className="space-child-main">
                  <div className="space-child-title">
                    <span>{child.name}</span>
                    {child.isDir && <span className="chip chip-neutral">目录</span>}
                    {child.safetyTier === "ask" && (
                      <span className="chip chip-ask">需确认</span>
                    )}
                    {child.isSynthetic && (
                      <span className="chip chip-neutral">未展开</span>
                    )}
                  </div>
                  <code title={child.path}>
                    {child.path || "其余未展示条目"}
                  </code>
                </div>

                <span className="space-child-size">{formatBytes(child.size)}</span>
                <span className="space-child-date">
                  {formatDate(child.modifiedMs)}
                </span>
                <span className="space-child-actions">
                  {child.isDir ? (
                    <>
                      {child.children.length > 0 ? (
                        <button
                          type="button"
                          className="btn small ghost"
                          onClick={() => openDir(child)}
                        >
                          打开
                        </button>
                      ) : (
                        <button
                          type="button"
                          className="btn small ghost"
                          onClick={() => void rescanDir(child)}
                          disabled={scanning || child.isSynthetic}
                        >
                          下钻
                        </button>
                      )}
                      {!child.isSynthetic && (
                        <button
                          type="button"
                          className="btn small ghost"
                          onClick={() => void reveal(child.path)}
                        >
                          <RevealIcon />
                        </button>
                      )}
                    </>
                  ) : (
                    <span className="space-readonly">
                      {child.deletable ? "可删除" : "只读"}
                    </span>
                  )}
                </span>
              </div>
            ))}
          </div>
        )}

        {currentNode && currentNode.children.length === 0 && (
          <div className="empty-state">
            <div className="empty-art" aria-hidden>
              <span />
              <span />
              <span />
            </div>
            <h3>{scanning ? "正在分析" : "这个目录没有展开的子项"}</h3>
            <p>
              {scanning
                ? "文件较多时需要一些时间，可以随时取消。"
                : "可能为空、无权限，或已达到展开深度。可以重新分析此目录，或返回上一层。"}
            </p>
            {!scanning && (
              <button
                type="button"
                className="btn ghost"
                onClick={() => void rescanDir(currentNode)}
              >
                重新分析此目录
              </button>
            )}
          </div>
        )}

        {!currentNode && !scanning && (
          <div className="empty-state">
            <div className="empty-art" aria-hidden>
              <span />
              <span />
              <span />
            </div>
            <h3>还没有空间分析结果</h3>
            <p>点击「分析主目录」，或选择一个自定义目录/外接卷开始。</p>
            <button
              type="button"
              className="btn ghost"
              onClick={() => void start()}
            >
              分析主目录
            </button>
          </div>
        )}
      </section>

      {selectedNodes.length > 0 && (
        <div className="actionbar">
          <div className="selection">
            <span className="selection-mark" aria-hidden />
            <span>
              已选 <strong>{formatCount(selectedNodes.length)}</strong> 个文件
            </span>
            <span className="selection-separator">·</span>
            <span className="selection-bytes">{formatBytes(selectedBytes)}</span>
          </div>
          <div className="actionbar-right">
            <span className="actionbar-hint">
              {hasAsk ? "包含需确认文件，必须逐项确认" : "目录节点不可删除"}
            </span>
            <button
              type="button"
              className="btn primary"
              onClick={() => setConfirmOpen(true)}
              disabled={scanning}
            >
              <TrashIcon />
              删除所选文件
            </button>
          </div>
        </div>
      )}

      <DeleteConfirmDialog
        open={confirmOpen}
        count={selectedNodes.length}
        totalBytes={selectedBytes}
        busy={deleting}
        title={hasAsk ? "确认删除需确认文件？" : "确认删除所选文件？"}
        notes={[
          "只删除你勾选的文件，不会删除目录或未选中的内容。",
          "默认移入废纸篓，可在废纸篓中「放回原处」恢复。",
          hasAsk
            ? "所选内容包含需确认区域，删除后可能无法恢复。"
            : "文件删除前仍会再次经过白名单与版本库校验。",
        ]}
        confirmLabel="删除所选文件"
        requireAcknowledgement={hasAsk}
        acknowledgementLabel="我已确认这些文件不再需要，并了解可能无法恢复。"
        danger={hasAsk}
        allowPermanent
        onCancel={() => setConfirmOpen(false)}
        onConfirm={(options) => void confirmDelete(options)}
      />
    </div>
  );
}
