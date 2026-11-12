import type { SpaceNode } from "../types";
import { formatBytes } from "../lib/format";
import { layoutTreemap } from "../lib/treemap";

const PALETTE = [
  "#0a6b5b",
  "#3b6ea5",
  "#b08a52",
  "#8a5a7a",
  "#5a7a3b",
  "#a15a3b",
  "#4a6b7a",
  "#7a6b3b",
];

interface SpaceTreemapProps {
  node: SpaceNode;
  selected: Set<string>;
  onOpenDir: (node: SpaceNode) => void;
  onRescanDir: (node: SpaceNode) => void;
  onToggleFile: (node: SpaceNode) => void;
}

function shorten(value: string, max: number): string {
  return value.length > max ? `${value.slice(0, max - 1)}…` : value;
}

export function SpaceTreemap({
  node,
  selected,
  onOpenDir,
  onRescanDir,
  onToggleFile,
}: SpaceTreemapProps) {
  const width = 1000;
  const height = 440;
  const cells = layoutTreemap(node.children, width, height);

  if (cells.length === 0) {
    return (
      <div className="treemap-empty">
        <p>这个目录没有可展示的子项。</p>
      </div>
    );
  }

  return (
    <div className="treemap-wrap">
      <svg
        className="treemap"
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label={`${node.name} 的空间树图`}
      >
        {cells.map((cell, index) => {
          const child = cell.node;
          const color = PALETTE[index % PALETTE.length];
          const isSelected = selected.has(child.id);
          const clickable =
            !child.isSynthetic &&
            (child.isDir || child.deletable);
          const cursor = !clickable
            ? "default"
            : child.isDir
              ? "zoom-in"
              : "pointer";
          const showName = cell.width > 74 && cell.height > 24;
          const showSize = cell.width > 96 && cell.height > 46;

          return (
            <g
              key={child.id}
              className={`treemap-cell${
                isSelected ? " is-selected" : ""
              }${child.isDir ? " is-dir" : ""}${
                child.isSynthetic ? " is-synthetic" : ""
              }`}
              style={{ cursor }}
              onClick={() => {
                if (child.isSynthetic) return;
                if (child.isDir) {
                  if (child.children.length > 0) {
                    onOpenDir(child);
                  } else {
                    onRescanDir(child);
                  }
                } else if (child.deletable) {
                  onToggleFile(child);
                }
              }}
            >
              <title>
                {child.name} · {formatBytes(child.size)}
                {child.isDir ? " · 点击下钻" : child.deletable ? " · 点击选择" : ""}
              </title>
              <rect
                x={cell.x + 1}
                y={cell.y + 1}
                width={Math.max(0, cell.width - 2)}
                height={Math.max(0, cell.height - 2)}
                rx={4}
                fill={color}
                fillOpacity={isSelected ? 0.34 : child.isDir ? 0.18 : 0.12}
                stroke={isSelected ? "var(--danger)" : color}
                strokeOpacity={isSelected ? 1 : 0.38}
                strokeWidth={isSelected ? 2.5 : 1}
              />
              {showName && (
                <text
                  x={cell.x + 9}
                  y={cell.y + 20}
                  className="treemap-label"
                  fill="var(--ink)"
                >
                  {shorten(child.name, Math.max(6, Math.floor(cell.width / 8)))}
                </text>
              )}
              {showSize && (
                <text
                  x={cell.x + 9}
                  y={cell.y + 37}
                  className="treemap-size"
                  fill="var(--muted)"
                >
                  {formatBytes(child.size)}
                </text>
              )}
            </g>
          );
        })}
      </svg>
    </div>
  );
}
