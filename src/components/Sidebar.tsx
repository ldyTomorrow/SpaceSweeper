import {
  AppsIcon,
  ChartIcon,
  CopyIcon,
  FileIcon,
  ScanIcon,
} from "./Icons";

export type PageKey =
  | "smart"
  | "space"
  | "large"
  | "duplicates"
  | "uninstaller";

interface SidebarItem {
  key: PageKey;
  label: string;
  hint: string;
  icon: typeof ScanIcon;
  planned?: string;
}

const ITEMS: SidebarItem[] = [
  { key: "smart", label: "智能扫描", hint: "缓存清理", icon: ScanIcon },
  { key: "space", label: "空间分析", hint: "目录树图", icon: ChartIcon, planned: "P1" },
  { key: "large", label: "大文件", hint: "大文件与旧文件", icon: FileIcon },
  { key: "duplicates", label: "重复文件", hint: "内容去重", icon: CopyIcon, planned: "P2" },
  { key: "uninstaller", label: "卸载器", hint: "应用残留", icon: AppsIcon, planned: "P3" },
];

interface SidebarProps {
  active: PageKey;
  onSelect: (page: PageKey) => void;
  version: string;
}

export function Sidebar({ active, onSelect, version }: SidebarProps) {
  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <span className="sidebar-kicker">WORKSPACE</span>
        <span className="sidebar-title">清理工作区</span>
      </div>

      <nav className="sidebar-nav" aria-label="主导航">
        {ITEMS.map((item) => {
          const Icon = item.icon;
          const isActive = item.key === active;
          const isPlanned = Boolean(item.planned);

          return (
            <button
              key={item.key}
              type="button"
              className={`sidebar-item${isActive ? " is-active" : ""}${
                isPlanned ? " is-planned" : ""
              }`}
              onClick={() => onSelect(item.key)}
              aria-current={isActive ? "page" : undefined}
              title={`${item.label} · ${item.hint}${item.planned ? `（${item.planned} 规划）` : ""}`}
            >
              <span className="sidebar-icon">
                <Icon size={17} />
              </span>
              <span className="sidebar-copy">
                <span className="sidebar-label">{item.label}</span>
                <span className="sidebar-hint">{item.hint}</span>
              </span>
              {item.planned && <span className="sidebar-badge">{item.planned}</span>}
            </button>
          );
        })}
      </nav>

      <div className="sidebar-footer">
        <span className="sidebar-footer-dot" aria-hidden />
        <span>本地运行 · 无遥测</span>
        <span className="sidebar-version">v{version}</span>
      </div>
    </aside>
  );
}
