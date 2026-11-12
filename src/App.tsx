import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { BigFilesPage } from "./pages/BigFilesPage";
import { DuplicatesPage } from "./pages/DuplicatesPage";
import { SmartScanPage } from "./pages/SmartScanPage";
import { SpaceAnalysisPage } from "./pages/SpaceAnalysisPage";
import { UninstallerPage } from "./pages/UninstallerPage";
import { Sidebar } from "./components/Sidebar";
import type { PageKey } from "./components/Sidebar";
import { AlertIcon, CloseIcon } from "./components/Icons";
import { toErrorMessage } from "./lib/format";

export default function App() {
  const [activePage, setActivePage] = useState<PageKey>("smart");
  const [fda, setFda] = useState<boolean | null>(null);
  const [shellError, setShellError] = useState<string | null>(null);

  const checkFda = useCallback(async () => {
    try {
      setFda(await invoke<boolean>("check_full_disk_access"));
    } catch {
      setFda(null);
    }
  }, []);

  useEffect(() => {
    void checkFda();
  }, [checkFda]);

  const openSettings = useCallback(() => {
    void invoke("open_full_disk_access_settings").catch((error) => {
      setShellError(toErrorMessage(error));
    });
  }, []);

  return (
    <div className="app">
      <Sidebar active={activePage} onSelect={setActivePage} version="0.2.0" />

      <div className="workspace">
        <header className="topbar">
          <div className="brand">
            <span className="brand-mark" aria-hidden />
            <div className="brand-copy">
              <span className="brand-name">SpaceSweeper</span>
              <span className="brand-tag">发现，再决定</span>
            </div>
          </div>

          <button
            type="button"
            className={`status-pill${fda === false ? " is-warn" : ""}`}
            onClick={() => void checkFda()}
            title="点击重新检测完全磁盘访问权限"
          >
            <span className="status-dot" aria-hidden />
            {fda === null
              ? "正在检测权限"
              : fda
                ? "完全磁盘访问已授权"
                : "完全磁盘访问未授权"}
          </button>
        </header>

        <main className="content">
          {shellError && (
            <div className="notice is-error">
              <span className="notice-icon">
                <AlertIcon size={20} />
              </span>
              <div className="notice-copy">
                <strong>操作没有完成</strong>
                <p>{shellError}</p>
              </div>
              <button
                type="button"
                className="icon-btn"
                onClick={() => setShellError(null)}
                aria-label="关闭错误提示"
              >
                <CloseIcon />
              </button>
            </div>
          )}

          <div
            className={`page-slot${activePage === "smart" ? "" : " is-hidden"}`}
          >
            <SmartScanPage
              fda={fda}
              onRecheck={() => void checkFda()}
              onOpenSettings={openSettings}
            />
          </div>

          <div
            className={`page-slot${activePage === "large" ? "" : " is-hidden"}`}
          >
            <BigFilesPage
              fda={fda}
              onRecheck={() => void checkFda()}
              onOpenSettings={openSettings}
            />
          </div>

          <div
            className={`page-slot${
              activePage === "duplicates" ? "" : " is-hidden"
            }`}
          >
            <DuplicatesPage
              fda={fda}
              onRecheck={() => void checkFda()}
              onOpenSettings={openSettings}
            />
          </div>

          <div className={`page-slot${activePage === "space" ? "" : " is-hidden"}`}>
            <SpaceAnalysisPage
              fda={fda}
              onRecheck={() => void checkFda()}
              onOpenSettings={openSettings}
            />
          </div>

          <div
            className={`page-slot${
              activePage === "uninstaller" ? "" : " is-hidden"
            }`}
          >
            <UninstallerPage
              fda={fda}
              onRecheck={() => void checkFda()}
              onOpenSettings={openSettings}
            />
          </div>
        </main>
      </div>
    </div>
  );
}
