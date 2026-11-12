export type SafetyTier = "safe" | "ask";

export interface CacheItem {
  id: string;
  path: string;
  name: string;
  size: number;
  fileCount: number;
  category: string;
  safetyTier: SafetyTier;
  isDir: boolean;
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

export interface PermanentTicket {
  token: string;
  expiresInSecs: number;
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

export interface SpaceNode {
  id: string;
  path: string;
  name: string;
  size: number;
  fileCount: number;
  isDir: boolean;
  safetyTier: SafetyTier | null;
  deletable: boolean;
  reason: string;
  modifiedMs: number | null;
  hiddenChildren: number;
  isSynthetic: boolean;
  children: SpaceNode[];
}

export interface SpaceScanSummary {
  root: string;
  filesScanned: number;
  bytesScanned: number;
  skippedEntries: number;
  durationMs: number;
  cancelled: boolean;
}

export interface SpaceAnalysis {
  tree: SpaceNode;
  summary: SpaceScanSummary;
}

export interface SpaceScanOptions {
  root?: string;
  maxDepth?: number;
  maxChildren?: number;
}

export type SpaceScanEvent =
  | { type: "started"; root: string }
  | {
      type: "progress";
      filesScanned: number;
      bytesScanned: number;
      skippedEntries: number;
      elapsedMs: number;
    };

export interface DuplicateFile {
  id: string;
  path: string;
  name: string;
  size: number;
  modifiedMs: number | null;
  isKeep: boolean;
}

export interface DuplicateGroup {
  id: string;
  size: number;
  wastedBytes: number;
  totalFiles: number;
  files: DuplicateFile[];
}

export interface DuplicateScanSummary {
  roots: string[];
  filesScanned: number;
  bytesScanned: number;
  filesHashed: number;
  bytesHashed: number;
  duplicateGroups: number;
  duplicateFiles: number;
  reclaimableBytes: number;
  skippedEntries: number;
  durationMs: number;
  cancelled: boolean;
}

export interface DuplicateScanOptions {
  roots?: string[];
  minSizeBytes?: number;
  maxGroups?: number;
}

export type DuplicateScanEvent =
  | { type: "started"; roots: string[]; minSizeBytes: number }
  | {
      type: "progress";
      filesScanned: number;
      filesHashed: number;
      bytesHashed: number;
      skippedEntries: number;
      elapsedMs: number;
    }
  | {
      type: "finished";
      cancelled: boolean;
      groups: DuplicateGroup[];
      summary: DuplicateScanSummary;
    };

export interface InstalledApp {
  id: string;
  path: string;
  name: string;
  bundleId: string | null;
  version: string | null;
  size: number;
  modifiedMs: number | null;
  isSystem: boolean;
  deletable: boolean;
  reason: string;
}

export interface AppResidual {
  id: string;
  path: string;
  name: string;
  size: number;
  category: string;
  safetyTier: SafetyTier;
  defaultSelected: boolean;
  reason: string;
}

export interface AppScanSummary {
  roots: string[];
  appsScanned: number;
  totalBytes: number;
  skippedEntries: number;
  durationMs: number;
  cancelled: boolean;
}

export type AppScanEvent =
  | { type: "started"; roots: string[] }
  | { type: "item"; app: InstalledApp }
  | {
      type: "progress";
      appsScanned: number;
      totalBytes: number;
      skippedEntries: number;
      elapsedMs: number;
    }
  | {
      type: "finished";
      cancelled: boolean;
      apps: InstalledApp[];
      summary: AppScanSummary;
    };
