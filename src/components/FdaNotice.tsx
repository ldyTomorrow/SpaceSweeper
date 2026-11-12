import { ShieldIcon } from "./Icons";

interface FdaNoticeProps {
  onRecheck: () => void;
  onOpenSettings: () => void;
}

export function FdaNotice({ onRecheck, onOpenSettings }: FdaNoticeProps) {
  return (
    <div className="notice">
      <span className="notice-icon">
        <ShieldIcon size={20} />
      </span>
      <div className="notice-copy">
        <strong>完全磁盘访问权限未开启</strong>
        <p>
          当前用户缓存与大文件扫描不受影响；扫描受 TCC 保护的目录时会跳过对应条目。
          可在「隐私与安全性 → 完全磁盘访问权限」中添加本应用，授权后重启应用。
        </p>
      </div>
      <div className="notice-actions">
        <button type="button" className="btn small ghost" onClick={onRecheck}>
          重新检测
        </button>
        <button type="button" className="btn small primary" onClick={onOpenSettings}>
          打开系统设置
        </button>
      </div>
    </div>
  );
}
