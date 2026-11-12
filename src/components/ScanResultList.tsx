import { useEffect, useRef, useState } from "react";
import type { CacheItem } from "../types";
import { formatBytes, formatCount, formatDate } from "../lib/format";
import { ChevronIcon, FileIcon, FolderIcon, ShieldIcon } from "./Icons";

interface ScanResultListProps {
  items: CacheItem[];
  selected: Set<string>;
  onToggle: (id: string) => void;
  onToggleAll: () => void;
  allowSelectAll?: boolean;
  toolbarHint?: string;
}

export function ScanResultList({
  items,
  selected,
  onToggle,
  onToggleAll,
  allowSelectAll = true,
  toolbarHint,
}: ScanResultListProps) {
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const selectAllRef = useRef<HTMLInputElement>(null);

  const allSelected = items.length > 0 && selected.size === items.length;
  const partiallySelected = selected.size > 0 && !allSelected;

  useEffect(() => {
    if (selectAllRef.current) {
      selectAllRef.current.indeterminate = partiallySelected;
    }
  }, [partiallySelected]);

  return (
    <div className="result-list">
      <div className="list-toolbar">
        {allowSelectAll ? (
          <label className="checkline">
            <span className="checkbox">
              <input
                ref={selectAllRef}
                type="checkbox"
                checked={allSelected}
                onChange={onToggleAll}
                aria-label="全选扫描结果"
              />
              <span className="box" aria-hidden />
            </span>
            全选
          </label>
        ) : (
          <span className="checkline">逐项选择</span>
        )}
        <span className="list-count">
          {formatCount(items.length)} 项 ·{" "}
          {toolbarHint ?? "safe 档默认勾选"}
        </span>
      </div>

      <div className="list-head" aria-hidden>
        <span />
        <span />
        <span>名称 / 路径</span>
        <span className="align-right">大小</span>
        <span>最后修改</span>
        <span />
      </div>

      <div role="list">
        {items.map((item, index) => {
          const checked = selected.has(item.id);
          const expanded = expandedId === item.id;
          const isDirectory = item.isDir;

          return (
            <div
              className={`row-group${checked ? "" : " is-unselected"}`}
              key={item.id}
              role="listitem"
              style={{ animationDelay: `${Math.min(index, 12) * 18}ms` }}
            >
              <div className="result-row">
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={() => onToggle(item.id)}
                    aria-label={`选择 ${item.name}`}
                  />
                  <span className="box" aria-hidden />
                </label>

                <span className="row-icon" aria-hidden>
                  {isDirectory ? <FolderIcon size={16} /> : <FileIcon size={16} />}
                </span>

                <div className="row-main">
                  <div className="row-title">
                    <span className="row-name" title={item.name}>
                      {item.name}
                    </span>
                    <span
                      className={`chip ${
                        item.safetyTier === "safe" ? "chip-safe" : "chip-ask"
                      }`}
                    >
                      {item.safetyTier === "safe" ? <ShieldIcon size={11} /> : null}
                      {item.safetyTier === "safe" ? "安全" : "需确认"}
                    </span>
                  </div>
                  <div className="row-path" title={item.path}>
                    {item.path}
                  </div>
                </div>

                <div className="row-size">{formatBytes(item.size)}</div>
                <div className="row-date">{formatDate(item.lastModified)}</div>

                <button
                  type="button"
                  className="row-expand"
                  onClick={() => setExpandedId(expanded ? null : item.id)}
                  aria-label={expanded ? "收起判定理由" : "展开判定理由"}
                  aria-expanded={expanded}
                >
                  <ChevronIcon className={expanded ? "rotated" : ""} />
                </button>
              </div>

              {expanded && (
                <div className="row-detail">
                  <div>
                    <span className="detail-label">判定理由</span>
                    <p>{item.reason}</p>
                  </div>
                  <div className="detail-meta">
                    <span>{formatCount(item.fileCount)} 个文件</span>
                    <span>{item.category}</span>
                    <span>{item.safetyTier === "safe" ? "safe 档" : "ask 档"}</span>
                  </div>
                  <div className="detail-path">
                    <span className="detail-label">完整路径</span>
                    <code>{item.path}</code>
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
