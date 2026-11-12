import { useCallback, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AppScanEvent,
  AppScanSummary,
  InstalledApp,
} from "../types";
import { toErrorMessage } from "../lib/format";

export type AppScanStatus = "idle" | "scanning" | "done" | "error";

export interface AppScanProgress {
  appsScanned: number;
  totalBytes: number;
  skippedEntries: number;
  elapsedMs: number;
}

export function useAppScan() {
  const [status, setStatus] = useState<AppScanStatus>("idle");
  const [apps, setApps] = useState<InstalledApp[]>([]);
  const [summary, setSummary] = useState<AppScanSummary | null>(null);
  const [progress, setProgress] = useState<AppScanProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  const inFlight = useRef(false);
  const scanIdRef = useRef<string | null>(null);

  const start = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;

    const scanId = `apps-${Date.now()}-${Math.random().toString(36).slice(2)}`;
    scanIdRef.current = scanId;

    setStatus("scanning");
    setApps([]);
    setSummary(null);
    setProgress(null);
    setError(null);

    const channel = new Channel<AppScanEvent>();
    channel.onmessage = (event) => {
      switch (event.type) {
        case "started":
          setProgress({
            appsScanned: 0,
            totalBytes: 0,
            skippedEntries: 0,
            elapsedMs: 0,
          });
          break;
        case "item":
          setApps((previous) =>
            previous.some((app) => app.id === event.app.id)
              ? previous
              : [...previous, event.app],
          );
          break;
        case "progress":
          setProgress({
            appsScanned: event.appsScanned,
            totalBytes: event.totalBytes,
            skippedEntries: event.skippedEntries,
            elapsedMs: event.elapsedMs,
          });
          break;
        case "finished":
          setApps(event.apps);
          setSummary(event.summary);
          setProgress({
            appsScanned: event.summary.appsScanned,
            totalBytes: event.summary.totalBytes,
            skippedEntries: event.summary.skippedEntries,
            elapsedMs: event.summary.durationMs,
          });
          setStatus("done");
          break;
      }
    };

    try {
      const result = await invoke<AppScanSummary>("scan_applications", {
        scanId,
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

  const removeApp = useCallback((path: string) => {
    setApps((previous) => previous.filter((app) => app.path !== path));
  }, []);

  const clearError = useCallback(() => setError(null), []);

  return {
    status,
    apps,
    summary,
    progress,
    error,
    start,
    cancel,
    removeApp,
    clearError,
  };
}
