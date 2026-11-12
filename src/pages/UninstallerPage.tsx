import { useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  AppResidual,
  DeleteReport,
  InstalledApp,
} from "../types";
import { useAppScan } from "../hooks/useAppScan";
import { DeleteConfirmDialog } from "../components/DeleteConfirmDialog";
import type { DeleteConfirmOptions } from "../components/DeleteConfirmDialog";
import { FdaNotice } from "../components/FdaNotice";
import {
  AlertIcon,
  AppsIcon,
  CheckIcon,
  CloseIcon,
  RevealIcon,
  ScanIcon,
  TrashIcon,
} from "../components/Icons";
import {
  displayHomePath,
  formatBytes,
  formatCount,
  toErrorMessage,
} from "../lib/format";

interface UninstallerPageProps {
  fda: boolean | null;
  onRecheck: () => void;
  onOpenSettings: () => void;
}

const NOTES = [
  "应用包会连同勾选的残留一起移入废纸篓，可在废纸篓中恢复。",
  "个人数据残留默认不勾选，必须逐项确认后才会卸载。",
  "系统应用与受保护应用不会提供卸载入口。",
];

export function UninstallerPage({
  fda,
  onRecheck,
  onOpenSettings,
}: UninstallerPageProps) {
  const scan = useAppScan();
  const {
    status,
    apps,
    summary,
    progress,
    error: scanError,
    start,
    cancel,
    removeApp,
    clearError,
  } = scan;

  const [search, setSearch] = useState("");
  const [selectedApp, setSelectedApp] = useState<InstalledApp | null>(null);
  const [residuals, setResiduals] = useState<AppResidual[]>([]);
  const [residualsLoading, setResidualsLoading] = useState(false);
  const [residualsError, setResidualsError] = useState<string | null>(null);
  const [selectedResiduals, setSelectedResiduals] = useState<Set<string>>(
    new Set(),
  );
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [report, setReport] = useState<DeleteReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const deleteInFlight = useRef(false);
  const residualRequest = useRef(0);

  const scanning = status === "scanning";
  const displayError = error ?? scanError;

  const filteredApps = useMemo(() => {
    const keyword = search.trim().toLowerCase();
    if (!keyword) return apps;
    return apps.filter(
      (app) =>
        app.name.toLowerCase().includes(keyword) ||
        (app.bundleId ?? "").toLowerCase().includes(keyword) ||
        app.path.toLowerCase().includes(keyword),
    );
  }, [apps, search]);

  const selectedResidualItems = useMemo(
    () => residuals.filter((residual) => selectedResiduals.has(residual.path)),
    [residuals, selectedResiduals],
  );
  const selectedResidualBytes = useMemo(
    () => selectedResidualItems.reduce((total, item) => total + item.size, 0),
    [selectedResidualItems],
  );
  const hasAsk = selectedResidualItems.some(
    (residual) => residual.safetyTier === "ask",
  );
  const totalBytes = summary?.totalBytes ?? 0;
  const durationText = summary
    ? summary.durationMs < 1000
      ? `${summary.durationMs} ms`
      : `${(summary.durationMs / 1000).toFixed(1)} s`
    : scanning
      ? "…"
      : "—";

  const selectApp = async (app: InstalledApp) => {
    setSelectedApp(app);
    setResiduals([]);
    setSelectedResiduals(new Set());
    setResidualsError(null);
    if (!app.deletable) return;

    const requestId = residualRequest.current + 1;
    residualRequest.current = requestId;
    setResidualsLoading(true);

    try {
      const items = await invoke<AppResidual[]>("find_app_residuals", {
        bundleId: app.bundleId,
        appName: app.name,
      });
      if (requestId !== residualRequest.current) return;
      setResiduals(items);
      setSelectedResiduals(
        new Set(
          items
            .filter((residual) => residual.defaultSelected)
            .map((residual) => residual.path),
        ),
      );
    } catch (residualError) {
      if (requestId !== residualRequest.current) return;
      setResidualsError(toErrorMessage(residualError));
    } finally {
      if (requestId === residualRequest.current) {
        setResidualsLoading(false);
      }
    }
  };

  const toggleResidual = (path: string) => {
    setSelectedResiduals((previous) => {
      const next = new Set(previous);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  };

  const reveal = async (path: string) => {
    try {
      await invoke("reveal_in_finder", { path });
    } catch (revealError) {
      setError(toErrorMessage(revealError));
    }
  };

  const confirmUninstall = async (_options: DeleteConfirmOptions) => {
    if (!selectedApp || deleteInFlight.current) return;
    deleteInFlight.current = true;
    setDeleting(true);

    try {
      const result = await invoke<DeleteReport>("uninstall_app", {
        request: {
          appPath: selectedApp.path,
          bundleId: selectedApp.bundleId,
          appName: selectedApp.name,
          residualPaths: Array.from(selectedResiduals),
          allowAsk: hasAsk,
        },
      });

      removeApp(selectedApp.path);
      setSelectedApp(null);
      setResiduals([]);
      setSelectedResiduals(new Set());
      setReport(result);
      setConfirmOpen(false);
    } catch (uninstallError) {
      setError(toErrorMessage(uninstallError));
      setConfirmOpen(false);
    } finally {
      deleteInFlight.current = false;
      setDeleting(false);
    }
  };

  const dismissError = () => {
    setError(null);
    clearError();
  };

  return (
    <div className="page">
      <section className="hero">
        <div className="hero-copy">
          <p className="eyebrow">UNINSTALLER · 卸载器</p>
          <h1>把应用和残留一起带走。</h1>
          <p className="lede">
            按 Bundle ID 匹配 Library 残留。安全缓存默认勾选，
            个人数据残留默认不勾选，系统应用不提供卸载入口。
          </p>
        </div>

        <div className="hero-actions">
          <button
            type="button"
            className="btn primary scan-btn"
            onClick={() => void start()}
            disabled={scanning}
          >
            {scanning ? <span className="spinner" aria-hidden /> : <ScanIcon />}
            {scanning ? "读取中…" : summary ? "重新扫描应用" : "扫描应用"}
          </button>
          <p className="scope">
            <code>/Applications · ~/Applications · /System/Applications</code>
            <span>只展示，不自动卸载 · 默认移入废纸篓</span>
          </p>
        </div>
      </section>

      {fda === false && (
        <FdaNotice onRecheck={onRecheck} onOpenSettings={onOpenSettings} />
      )}

      {displayError && (
        <div className="notice is-error">
          <span className="notice-icon">
            <AlertIcon size={20} />
          </span>
          <div className="notice-copy">
            <strong>操作没有完成</strong>
            <p>{displayError}</p>
          </div>
          <button
            type="button"
            className="icon-btn"
            onClick={dismissError}
            aria-label="关闭错误提示"
          >
            <CloseIcon />
          </button>
        </div>
      )}

      {report && (
        <div className="notice is-success">
          <span className="notice-icon">
            <CheckIcon size={20} />
          </span>
          <div className="notice-copy">
            <strong>
              {report.removed.length > 0
                ? `已移入废纸篓 ${formatBytes(report.freedBytes)}`
                : "没有项目被移入废纸篓"}
            </strong>
            <p>
              {report.removed.length > 0
                ? `${formatCount(report.removed.length)} 项已处理，可在废纸篓中恢复；清空废纸篓后释放空间。`
                : "应用或残留被安全守卫跳过，未发生删除。"}
            </p>
            {report.skipped.length > 0 && (
              <details className="report-details">
                <summary>
                  {formatCount(report.skipped.length)} 项未处理，查看原因
                </summary>
                <ul>
                  {report.skipped.map((entry) => (
                    <li key={entry.path}>
                      <code>{entry.path}</code>
                      <span>{entry.reason}</span>
                    </li>
                  ))}
                </ul>
              </details>
            )}
          </div>
          <button
            type="button"
            className="icon-btn"
            onClick={() => setReport(null)}
            aria-label="关闭卸载结果"
          >
            <CloseIcon />
          </button>
        </div>
      )}

      {scanning && (
        <div className="scan-progress">
          <div className="progress-track is-indeterminate">
            <span />
          </div>
          <span className="progress-label">
            {formatCount(progress?.appsScanned ?? 0)} 个应用 ·{" "}
            {formatBytes(progress?.totalBytes ?? 0)}
          </span>
          <button
            type="button"
            className="btn small ghost"
            onClick={() => void cancel()}
          >
            取消
          </button>
        </div>
      )}

      <section className="metric-strip" aria-label="应用扫描统计">
        <div className="metric hero-metric">
          <span className="metric-label">应用总占用</span>
          <strong>{formatBytes(totalBytes)}</strong>
        </div>
        <div className="metric">
          <span className="metric-label">应用数量</span>
          <strong>{formatCount(apps.length)}</strong>
        </div>
        <div className="metric">
          <span className="metric-label">系统 / 受保护</span>
          <strong>{formatCount(apps.filter((app) => !app.deletable).length)}</strong>
        </div>
        <div className="metric">
          <span className="metric-label">耗时</span>
          <strong>{durationText}</strong>
        </div>
      </section>

      <section className="panel">
        <div className="panel-head">
          <div>
            <h2>
              已安装应用
              {apps.length > 0 && (
                <span className="count-badge">{formatCount(apps.length)}</span>
              )}
            </h2>
            <p>
              {scanning
                ? "正在读取应用包与大小，结果会逐个出现。"
                : summary
                  ? `扫描完成 · ${summary.roots.length} 个应用目录`
                  : "扫描后选择应用，查看可清理的残留。"}
            </p>
          </div>
          <input
            type="search"
            className="search-input"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder="搜索应用名或 Bundle ID"
            aria-label="搜索应用"
          />
        </div>

        {apps.length > 0 ? (
          <div className="uninstaller-layout">
            <div className="app-list" role="list">
              {filteredApps.map((app) => (
                <button
                  type="button"
                  className={`app-row${
                    selectedApp?.path === app.path ? " is-active" : ""
                  }${app.deletable ? "" : " is-protected"}`}
                  key={app.id}
                  onClick={() => void selectApp(app)}
                  role="listitem"
                >
                  <span className="app-icon" aria-hidden>
                    <AppsIcon size={17} />
                  </span>
                  <span className="app-main">
                    <span className="app-title">
                      {app.name}
                      {app.isSystem && <span className="chip chip-neutral">系统</span>}
                      {!app.deletable && !app.isSystem && (
                        <span className="chip chip-ask">受保护</span>
                      )}
                    </span>
                    <span className="app-meta">
                      {app.bundleId ?? "无 Bundle ID"}
                      {app.version ? ` · ${app.version}` : ""}
                    </span>
                  </span>
                  <span className="app-size">{formatBytes(app.size)}</span>
                </button>
              ))}
              {filteredApps.length === 0 && (
                <p className="app-empty">没有匹配的应用。</p>
              )}
            </div>

            <div className="app-detail">
              {selectedApp ? (
                <>
                  <div className="app-detail-head">
                    <div>
                      <h3>{selectedApp.name}</h3>
                      <p>
                        {selectedApp.bundleId ?? "无 Bundle ID"}
                        {selectedApp.version ? ` · ${selectedApp.version}` : ""}
                      </p>
                    </div>
                    <span className="app-detail-size">
                      {formatBytes(selectedApp.size)}
                    </span>
                  </div>

                  <div className="app-path">
                    <code title={selectedApp.path}>
                      {displayHomePath(selectedApp.path)}
                    </code>
                    <button
                      type="button"
                      className="btn small ghost"
                      onClick={() => void reveal(selectedApp.path)}
                    >
                      <RevealIcon />
                      定位
                    </button>
                  </div>

                  {!selectedApp.deletable ? (
                    <div className="app-protected-note">
                      <AlertIcon size={18} />
                      <p>{selectedApp.reason}</p>
                    </div>
                  ) : (
                    <>
                      <div className="residual-head">
                        <span>应用残留</span>
                        <span>
                          已选 {formatCount(selectedResidualItems.length)} 项 ·{" "}
                          {formatBytes(selectedResidualBytes)}
                        </span>
                      </div>

                      {residualsLoading && (
                        <p className="residual-loading">
                          <span className="spinner" aria-hidden /> 正在读取 Library 残留…
                        </p>
                      )}

                      {residualsError && (
                        <p className="residual-error">{residualsError}</p>
                      )}

                      {!residualsLoading && residuals.length === 0 && !residualsError && (
                        <p className="residual-empty">
                          没有发现可识别的残留。卸载时只会移除应用包。
                        </p>
                      )}

                      {residuals.length > 0 && (
                        <div className="residual-list">
                          {residuals.map((residual) => (
                            <label
                              className={`residual-row${
                                residual.safetyTier === "ask" ? " is-ask" : ""
                              }`}
                              key={residual.id}
                            >
                              <span className="checkbox">
                                <input
                                  type="checkbox"
                                  checked={selectedResiduals.has(residual.path)}
                                  onChange={() => toggleResidual(residual.path)}
                                />
                                <span className="box" aria-hidden />
                              </span>
                              <span className="residual-main">
                                <span className="residual-name">
                                  {residual.name}
                                  <span
                                    className={`chip ${
                                      residual.safetyTier === "safe"
                                        ? "chip-safe"
                                        : "chip-ask"
                                    }`}
                                  >
                                    {residual.safetyTier === "safe"
                                      ? residual.category
                                      : "个人数据"}
                                  </span>
                                </span>
                                <code title={residual.path}>{residual.path}</code>
                              </span>
                              <span className="residual-size">
                                {formatBytes(residual.size)}
                              </span>
                            </label>
                          ))}
                        </div>
                      )}

                      <div className="app-detail-actions">
                        <span className="actionbar-hint">
                          {hasAsk
                            ? "包含个人数据残留，必须逐项确认"
                            : "默认仅勾选可再生缓存与日志"}
                        </span>
                        <button
                          type="button"
                          className="btn primary"
                          onClick={() => setConfirmOpen(true)}
                          disabled={residualsLoading || deleting}
                        >
                          <TrashIcon />
                          卸载应用
                        </button>
                      </div>
                    </>
                  )}
                </>
              ) : (
                <div className="app-detail-empty">
                  <AppsIcon size={28} />
                  <p>从左侧选择一个应用，查看应用信息与可清理残留。</p>
                </div>
              )}
            </div>
          </div>
        ) : (
          <div className="empty-state">
            <div className="empty-art" aria-hidden>
              <span />
              <span />
              <span />
            </div>
            <h3>
              {scanning
                ? "正在读取应用目录"
                : status === "done"
                  ? "没有发现应用"
                  : "还没有应用列表"}
            </h3>
            <p>
              {scanning
                ? "应用包大小计算可能需要一些时间，结果会逐个出现。"
                : status === "done"
                  ? "可以重新扫描，或确认应用目录是否可读。"
                  : "点击「扫描应用」后开始，系统应用只展示不可卸载。"}
            </p>
            {!scanning && (
              <button
                type="button"
                className="btn ghost"
                onClick={() => void start()}
              >
                {summary ? "重新扫描应用" : "扫描应用"}
              </button>
            )}
          </div>
        )}
      </section>

      <DeleteConfirmDialog
        open={confirmOpen}
        count={selectedResidualItems.length + 1}
        totalBytes={(selectedApp?.size ?? 0) + selectedResidualBytes}
        busy={deleting}
        title="确认卸载应用？"
        notes={NOTES}
        confirmLabel="卸载并移入废纸篓"
        requireAcknowledgement={hasAsk}
        acknowledgementLabel="我确认勾选的个人数据残留不再需要，并了解卸载后可能无法恢复。"
        danger={hasAsk}
        onCancel={() => setConfirmOpen(false)}
        onConfirm={(options) => void confirmUninstall(options)}
      />
    </div>
  );
}
