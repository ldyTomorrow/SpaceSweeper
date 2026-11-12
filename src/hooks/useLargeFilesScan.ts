import { useCallback, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  LargeFileItem,
  LargeScanEvent,
  LargeScanOptions,
  LargeScanSummary,
} from "../types";
import { toErrorMessage } from "../lib/format";

export type LargeScanStatus = "idle" | "scanning" | "done" | "error";

export interface LargeScanProgress {
  filesScanned: number;
  bytesScanned: number;
  skippedEntries: number;
  elapsedMs: number;
}

export function useLargeFilesScan() {
  const [status, setStatus] = useState<LargeScanStatus>("idle");
  const [big, setBig] = useState<LargeFileItem[]>([]);
  const [old, setOld] = useState<LargeFileItem[]>([]);
  const [installers, setInstallers] = useState<LargeFileItem[]>([]);
  const [summary, setSummary] = useState<LargeScanSummary | null>(null);
  const [progress, setProgress] = useState<LargeScanProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  const inFlight = useRef(false);
  const scanIdRef = useRef<string | null>(null);

  const start = useCallback(async (options: LargeScanOptions) => {
    if (inFlight.current) return;
    inFlight.current = true;

    const scanId = `large-${Date.now()}-${Math.random().toString(36).slice(2)}`;
    scanIdRef.current = scanId;

    setStatus("scanning");
    setBig([]);
    setOld([]);
    setInstallers([]);
    setSummary(null);
    setProgress(null);
    setError(null);

    const channel = new Channel<LargeScanEvent>();
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
        case "finished":
          setBig(event.big);
          setOld(event.old);
          setInstallers(event.installers);
          setSummary(event.summary);
          setProgress({
            filesScanned: event.summary.filesScanned,
            bytesScanned: event.summary.bytesScanned,
            skippedEntries: event.summary.skippedEntries,
            elapsedMs: event.summary.durationMs,
          });
          setStatus("done");
          break;
      }
    };

    try {
      const result = await invoke<LargeScanSummary>("scan_large_files", {
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
    const keep = (item: LargeFileItem) => !paths.has(item.path);
    setBig((previous) => previous.filter(keep));
    setOld((previous) => previous.filter(keep));
    setInstallers((previous) => previous.filter(keep));
  }, []);

  const clearError = useCallback(() => setError(null), []);

  return {
    status,
    big,
    old,
    installers,
    summary,
    progress,
    error,
    start,
    cancel,
    removePaths,
    clearError,
  };
}
