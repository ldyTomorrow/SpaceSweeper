import { useCallback, useEffect, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  SpaceAnalysis,
  SpaceNode,
  SpaceScanEvent,
  SpaceScanSummary,
} from "../types";
import { toErrorMessage } from "../lib/format";

export type SpaceScanStatus = "idle" | "scanning" | "done" | "error";

export interface SpaceProgress {
  filesScanned: number;
  bytesScanned: number;
  skippedEntries: number;
  elapsedMs: number;
}

function pruneTree(
  node: SpaceNode,
  removed: Set<string>,
): { node: SpaceNode | null; sizeDelta: number; fileDelta: number } {
  if (removed.has(node.path)) {
    return { node: null, sizeDelta: node.size, fileDelta: node.fileCount };
  }

  let sizeDelta = 0;
  let fileDelta = 0;
  const children: SpaceNode[] = [];

  for (const child of node.children) {
    const result = pruneTree(child, removed);
    sizeDelta += result.sizeDelta;
    fileDelta += result.fileDelta;
    if (result.node) children.push(result.node);
  }

  if (sizeDelta === 0 && fileDelta === 0) {
    return { node, sizeDelta: 0, fileDelta: 0 };
  }

  return {
    node: {
      ...node,
      size: Math.max(0, node.size - sizeDelta),
      fileCount: Math.max(0, node.fileCount - fileDelta),
      children,
    },
    sizeDelta,
    fileDelta,
  };
}

export function useSpaceAnalysis() {
  const [status, setStatus] = useState<SpaceScanStatus>("idle");
  const [tree, setTree] = useState<SpaceNode | null>(null);
  const [summary, setSummary] = useState<SpaceScanSummary | null>(null);
  const [progress, setProgress] = useState<SpaceProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [home, setHome] = useState<string | null>(null);

  const inFlight = useRef(false);
  const scanIdRef = useRef<string | null>(null);

  useEffect(() => {
    void invoke<string>("home_directory")
      .then(setHome)
      .catch(() => setHome(null));
  }, []);

  const start = useCallback(async (root?: string) => {
    if (inFlight.current) return;
    inFlight.current = true;

    const scanId = `space-${Date.now()}-${Math.random().toString(36).slice(2)}`;
    scanIdRef.current = scanId;

    setStatus("scanning");
    setTree(null);
    setSummary(null);
    setProgress(null);
    setError(null);

    const channel = new Channel<SpaceScanEvent>();
    channel.onmessage = (event) => {
      switch (event.type) {
        case "started":
          setProgress({
            filesScanned: 0,
            bytesScanned: 0,
            skippedEntries: 0,
            elapsedMs: 0,
          });
          break;
        case "progress":
          setProgress({
            filesScanned: event.filesScanned,
            bytesScanned: event.bytesScanned,
            skippedEntries: event.skippedEntries,
            elapsedMs: event.elapsedMs,
          });
          break;
      }
    };

    try {
      const result = await invoke<SpaceAnalysis>("analyze_space", {
        scanId,
        options: { root, maxDepth: 4, maxChildren: 18 },
        onEvent: channel,
      });
      setTree(result.tree);
      setSummary(result.summary);
      setStatus("done");
    } catch (scanError) {
      setError(toErrorMessage(scanError));
      setStatus("error");
    } finally {
      inFlight.current = false;
      scanIdRef.current = null;
    }
  }, []);

  const cancel = useCallback(async () => {
    const scanId = scanIdRef.current;
    if (!scanId) return;
    try {
      await invoke<boolean>("cancel_scan", { scanId });
    } catch (cancelError) {
      setError(toErrorMessage(cancelError));
    }
  }, []);

  const chooseAndAnalyze = useCallback(async () => {
    try {
      const path = await invoke<string | null>("choose_directory");
      if (!path) return;

      let target = path;
      setNotice(null);
      try {
        target = await invoke<string>("authorize_scan_root", { path });
        setNotice("已授权该目录用于本次会话删除；重启应用后授权自动失效。");
      } catch (authorizeError) {
        setNotice(
          `该目录仅支持查看，不能删除：${toErrorMessage(authorizeError)}`,
        );
      }
      await start(target);
    } catch (chooseError) {
      setError(toErrorMessage(chooseError));
    }
  }, [start]);

  const removePaths = useCallback((paths: Set<string>) => {
    setTree((previous) => {
      if (!previous) return previous;
      const result = pruneTree(previous, paths);
      return result.node;
    });
  }, []);

  const clearError = useCallback(() => setError(null), []);
  const clearNotice = useCallback(() => setNotice(null), []);

  return {
    status,
    tree,
    summary,
    progress,
    error,
    notice,
    home,
    start,
    cancel,
    chooseAndAnalyze,
    removePaths,
    clearError,
    clearNotice,
  };
}
