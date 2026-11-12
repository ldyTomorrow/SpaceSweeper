import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { BigFilesPage } from "./pages/BigFilesPage";
import { ComingSoonPage } from "./pages/ComingSoonPage";
import { SmartScanPage } from "./pages/SmartScanPage";
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

  const renderPlannedPage = () => {
    switch (activePage) {
      case "space":
        return (
          <ComingSoonPage
            eyebrow="SPACE LENS · 空间分析"
            title="从树图看见空间去了哪里。"
            description="按目录聚合大小，逐层下钻；权限不足的目录会优雅跳过，不阻塞其他结果。"
            phase="P1 规划"
          />
        );
      case "duplicates":
        return (
          <ComingSoonPage
            eyebrow="DUPLICATES · 重复文件"
            title="相同内容，只保留一份。"
            description="大小分组、前 4 KB 哈希、完整哈希三段式检测；每组保留项不可勾选。"
            phase="P2 规划"
          />
        );
      case "uninstaller":
        return (
          <ComingSoonPage
            eyebrow="UNINSTALLER · 卸载器"
            title="把应用和残留一起带走。"
            description="按 Bundle ID 匹配应用残留，安全档默认勾选，个人数据残留必须逐项确认。"
            phase="P3 规划"
          />
        );
      default:
        return null;
    }
  };

  return (
    <div className="app">
      <Sidebar active={activePage} onSelect={setActivePage} version="0.1.0" />

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

          {renderPlannedPage()}
        </main>
      </div>
    </div>
  );
}
