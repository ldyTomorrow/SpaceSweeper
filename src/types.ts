export type SafetyTier = "safe" | "ask";

export interface CacheItem {
  id: string;
  path: string;
  name: string;
  size: number;
  fileCount: number;
  category: string;
  safetyTier: SafetyTier;
  lastModified: number | null;
  reason: string;
}

export type ScanEvent =
  | { type: "started"; root: string; totalEntries: number }
  | { type: "item"; item: CacheItem }
  | {
      type: "progress";
      itemsDone: number;
      itemsTotal: number;
      fileCount: number;
      totalBytes: number;
    }
  | {
      type: "finished";
      itemCount: number;
      fileCount: number;
      totalBytes: number;
      skippedEntries: number;
      deniedEntries: number;
      durationMs: number;
    };

export interface ScanSummary {
  root: string;
  itemCount: number;
  fileCount: number;
  totalBytes: number;
  skippedEntries: number;
  deniedEntries: number;
  durationMs: number;
}

export interface RemovedEntry {
  path: string;
  size: number;
}

export interface SkippedEntry {
  path: string;
  reason: string;
}

export interface DeleteReport {
  mode: "trash" | "permanent";
  removed: RemovedEntry[];
  skipped: SkippedEntry[];
  freedBytes: number;
}

export type LargeFileCategory = "big" | "old" | "installer";

export interface LargeFileItem {
  id: string;
  path: string;
  name: string;
  size: number;
  modifiedMs: number | null;
  ageDays: number;
  category: LargeFileCategory;
  reason: string;
}

export interface LargeScanSummary {
  roots: string[];
  filesScanned: number;
  bytesScanned: number;
  skippedEntries: number;
  durationMs: number;
  cancelled: boolean;
}

export interface LargeScanOptions {
  roots?: string[];
  minSizeBytes?: number;
  oldDays?: number;
  installerDays?: number;
  topN?: number;
}

export type LargeScanEvent =
  | {
      type: "started";
      roots: string[];
      minSizeBytes: number;
      oldDays: number;
      installerDays: number;
      topN: number;
    }
  | {
      type: "progress";
      filesScanned: number;
      bytesScanned: number;
      skippedEntries: number;
      elapsedMs: number;
    }
  | {
      type: "finished";
      cancelled: boolean;
      big: LargeFileItem[];
      old: LargeFileItem[];
      installers: LargeFileItem[];
      summary: LargeScanSummary;
    };
