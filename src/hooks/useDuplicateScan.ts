import { useCallback, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  DuplicateGroup,
  DuplicateScanEvent,
  DuplicateScanOptions,
  DuplicateScanSummary,
} from "../types";
import { toErrorMessage } from "../lib/format";

export type DuplicateScanStatus = "idle" | "scanning" | "done" | "error";

export interface DuplicateProgress {
  filesScanned: number;
  filesHashed: number;
  bytesHashed: number;
  skippedEntries: number;
  elapsedMs: number;
}

export function useDuplicateScan() {
  const [status, setStatus] = useState<DuplicateScanStatus>("idle");
  const [groups, setGroups] = useState<DuplicateGroup[]>([]);
  const [summary, setSummary] = useState<DuplicateScanSummary | null>(null);
  const [progress, setProgress] = useState<DuplicateProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  const inFlight = useRef(false);
  const scanIdRef = useRef<string | null>(null);

  const start = useCallback(async (options: DuplicateScanOptions) => {
    if (inFlight.current) return;
    inFlight.current = true;

    const scanId = `duplicates-${Date.now()}-${Math.random()
      .toString(36)
      .slice(2)}`;
    scanIdRef.current = scanId;

    setStatus("scanning");
    setGroups([]);
    setSummary(null);
    setProgress(null);
    setError(null);

    const channel = new Channel<DuplicateScanEvent>();
    channel.onmessage = (event) => {
      switch (event.type) {
        case "started":
          setProgress({
            filesScanned: 0,
            filesHashed: 0,
            bytesHashed: 0,
            skippedEntries: 0,
            elapsedMs: 0,
          });
          break;
        case "progress":
          setProgress({
            filesScanned: event.filesScanned,
            filesHashed: event.filesHashed,
            bytesHashed: event.bytesHashed,
            skippedEntries: event.skippedEntries,
            elapsedMs: event.elapsedMs,
          });
          break;
        case "finished":
          setGroups(event.groups);
          setSummary(event.summary);
          setProgress({
            filesScanned: event.summary.filesScanned,
            filesHashed: event.summary.filesHashed,
            bytesHashed: event.summary.bytesHashed,
            skippedEntries: event.summary.skippedEntries,
            elapsedMs: event.summary.durationMs,
          });
          setStatus("done");
          break;
      }
    };

    try {
      const result = await invoke<DuplicateScanSummary>("scan_duplicates", {
        scanId,
        options,
        onEvent: channel,
      });
      setSummary(result);
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

  const removePaths = useCallback((paths: Set<string>) => {
    setGroups((previous) =>
      previous
        .map((group) => ({
          ...group,
          files: group.files.filter((file) => !paths.has(file.path)),
        }))
        .filter((group) => group.files.length >= 2),
    );
  }, []);

  const clearError = useCallback(() => setError(null), []);

  return {
    status,
    groups,
    summary,
    progress,
    error,
    start,
    cancel,
    removePaths,
    clearError,
  };
}
