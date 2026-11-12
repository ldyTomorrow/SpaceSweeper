import type { SpaceNode } from "../types";

export interface TreemapCell {
  node: SpaceNode;
  x: number;
  y: number;
  width: number;
  height: number;
}

const MIN_CELL = 2;

/**
 * 简单二分树图：按大小降序，沿较长边切分，递归到叶子。
 * 不依赖第三方布局库，结果稳定且可点击。
 */
export function layoutTreemap(
  nodes: SpaceNode[],
  width: number,
  height: number,
): TreemapCell[] {
  const items = nodes
    .filter((node) => node.size > 0)
    .sort((left, right) => right.size - left.size);
  if (items.length === 0 || width <= 0 || height <= 0) return [];

  const cells: TreemapCell[] = [];
  place(items, 0, 0, width, height, cells);
  return cells;
}

function place(
  items: SpaceNode[],
  x: number,
  y: number,
  width: number,
  height: number,
  cells: TreemapCell[],
): void {
  if (items.length === 0 || width < MIN_CELL || height < MIN_CELL) return;

  if (items.length === 1) {
    cells.push({ node: items[0], x, y, width, height });
    return;
  }

  const total = items.reduce((sum, node) => sum + node.size, 0);
  if (total <= 0) return;

  let accumulated = 0;
  let split = 1;
  for (let index = 0; index < items.length - 1; index += 1) {
    accumulated += items[index].size;
    if (accumulated >= total / 2) {
      split = index + 1;
      break;
    }
    split = index + 2;
  }

  const first = items.slice(0, split);
  const second = items.slice(split);
  if (second.length === 0) {
    cells.push({ node: items[0], x, y, width, height });
    return;
  }

  const firstSize = first.reduce((sum, node) => sum + node.size, 0);
  const ratio = Math.min(0.95, Math.max(0.05, firstSize / total));

  if (width >= height) {
    const firstWidth = width * ratio;
    place(first, x, y, firstWidth, height, cells);
    place(second, x + firstWidth, y, width - firstWidth, height, cells);
  } else {
    const firstHeight = height * ratio;
    place(first, x, y, width, firstHeight, cells);
    place(second, x, y + firstHeight, width, height - firstHeight, cells);
  }
}
