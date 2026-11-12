import { useEffect, useRef, useState } from "react";
import type { LargeFileItem } from "../types";
import { formatAge, formatBytes, formatCount, formatDate } from "../lib/format";
import { ChevronIcon, FileIcon, RevealIcon } from "./Icons";

const CATEGORY_LABEL: Record<LargeFileItem["category"], string> = {
  big: "大文件",
  old: "旧文件",
  installer: "旧安装包",
};

interface LargeFileListProps {
  items: LargeFileItem[];
  selected: Set<string>;
  onToggle: (id: string) => void;
  onToggleAll: () => void;
  onReveal: (path: string) => void;
}

export function LargeFileList({
  items,
  selected,
  onToggle,
  onToggleAll,
  onReveal,
}: LargeFileListProps) {
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
        <label className="checkline">
          <span className="checkbox">
            <input
              ref={selectAllRef}
              type="checkbox"
              checked={allSelected}
              onChange={onToggleAll}
              aria-label="全选当前分组"
            />
            <span className="box" aria-hidden />
          </span>
          全选
        </label>
        <span className="list-count">
          {formatCount(items.length)} 项 · 仅显示符合条件的 Top 50
        </span>
      </div>

      <div className="list-head" aria-hidden>
        <span />
        <span />
        <span>名称 / 路径</span>
        <span className="align-right">大小</span>
        <span>年龄</span>
        <span />
      </div>

      <div role="list">
        {items.map((item, index) => {
          const checked = selected.has(item.id);
          const expanded = expandedId === item.id;

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
                  <FileIcon size={16} />
                </span>

                <div className="row-main">
                  <div className="row-title">
                    <span className="row-name" title={item.name}>
                      {item.name}
                    </span>
                    <span className="chip chip-neutral">
                      {CATEGORY_LABEL[item.category]}
                    </span>
                  </div>
                  <div className="row-path" title={item.path}>
                    {item.path}
                  </div>
                </div>

                <div className="row-size">{formatBytes(item.size)}</div>
                <div className="row-date" title={formatDate(item.modifiedMs)}>
                  {formatAge(item.ageDays)}
                </div>

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
                    <span>{CATEGORY_LABEL[item.category]}</span>
                    <span>最后修改 {formatDate(item.modifiedMs)}</span>
                    <span>{formatCount(item.ageDays)} 天</span>
                  </div>
                  <div className="detail-path">
                    <span className="detail-label">完整路径</span>
                    <code>{item.path}</code>
                  </div>
                  <div className="detail-actions">
                    <button
                      type="button"
                      className="btn small ghost"
                      onClick={() => onReveal(item.path)}
                    >
                      <RevealIcon />
                      在 Finder 中显示
                    </button>
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
