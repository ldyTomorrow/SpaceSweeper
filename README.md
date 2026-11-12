<p align="center">
  <img src="src-tauri/icons/icon.png" width="112" alt="SpaceSweeper app icon" />
</p>

<h1 align="center">SpaceSweeper</h1>

<p align="center">macOS 磁盘空间发现与安全回收工具</p>

<p align="center">
  <a href="#中文">中文</a> · <a href="#english">English</a>
</p>

> 先看清，再决定。  
> 一个独立的 macOS 文件发现与安全回收工具。

SpaceSweeper 是一个独立开发的原创 macOS 桌面工具，基于 **Tauri 2 + Rust + React/TypeScript**，只做两件事：

1. 找到用户可能不再需要的文件；
2. 用透明、可逆的方式删除它们。

它不做病毒查杀、恶意软件检测、性能优化、启动项管理，也不读取浏览器历史等隐私数据。

## 中文

### 项目定位

SpaceSweeper 的差异化不在“清理得多”，而在 **发现透明、删除可逆**：

- 每条结果都能展开查看完整路径、大小、修改时间与判定理由；
- 所有删除默认移入废纸篓，可在 Finder 中「放回原处」恢复；
- 不存在“一键静默清理全部结果”的入口；
- 需要用户确认的内容不会默认勾选。

### 当前功能

当前版本为 `0.1.0`，已完成 P0 闭环：

| 模块 | 能力 |
| --- | --- |
| 智能扫描 | 扫描 `~/Library/Caches`，并行计算条目大小，通过 Tauri Channel 实时推送结果 |
| 大文件 | 扫描 `~/Downloads`、`~/Desktop`、`~/Documents`、`~/Movies`，默认展示 ≥100 MB 的 Top 50 |
| 旧文件 | ≥100 MB 且 ≥365 天未修改，按“大小 × 陈旧天数”排序，Top 50 |
| 旧安装包 | `.dmg` / `.pkg` / `.zip`，≥50 MB 且 ≥180 天，Top 50 |
| 安全删除 | 默认移入废纸篓；删除前再次执行白名单与版本库校验 |
| 定位 | 在 Finder 中显示所选文件 |
| 扫描控制 | 进度展示、取消扫描，取消后可查看已发现的部分结果 |
| 权限引导 | 未授予完全磁盘访问权限时展示引导，不静默失败 |

侧边栏还提供空间分析、重复文件、卸载器入口，目前为 P1/P2/P3 规划占位。

### 安全设计

删除安全是本项目的第一优先级，核心守卫位于 [`safety.rs`](src-tauri/src/safety.rs)：

- **白名单根目录**：只允许删除固定用户目录与缓存/日志白名单内的内容；
- **规范化路径**：先 `canonicalize` 再比较，防止父级符号链接逃逸；
- **拒绝危险路径**：`..`、符号链接、白名单根目录本身、`/System`、`/usr`、`/bin` 等系统路径全部拒绝；
- **版本库保护**：扫描阶段跳过包含 `.git` 的仓库根目录；删除阶段向上检查祖先目录，仓库内文件即使被前端传入也会被拒绝；
- **运行中应用保护**：直接以 Bundle ID 命名的缓存目录在对应 App 运行时跳过；
- **默认废纸篓**：`cleaner.rs` 在 macOS 上通过 `NSFileManager` 移入废纸篓；
- **永久删除默认不开放**：后端实现了 5 分钟有效、一次性、与路径清单绑定的一次性令牌，但当前 UI 不提供入口；
- **审计日志**：删除结果逐项写入 `~/Library/Logs/SpaceSweeper/audit.log`；
- **无遥测、无自动清理、无后台常驻扫描**。

「需确认」档路径（Xcode DeviceSupport、CoreSimulator Devices、Docker 数据、Ollama/HuggingFace 模型缓存、iOS 备份）已在后端白名单中预留，但当前 P0 UI 尚未开放这些入口。

### 架构

```mermaid
flowchart LR
  UI["React + TypeScript UI"] -->|"Tauri IPC / Channel"| CMD["Rust commands"]
  CMD --> SCAN["scanner/<br/>cache · large_files"]
  CMD --> SAFETY["safety.rs<br/>whitelist · git guard"]
  CMD --> CLEANER["cleaner.rs<br/>trash · audit"]
  SCAN --> FS["macOS filesystem metadata"]
  CLEANER --> TRASH["Trash（可恢复）"]
  CLEANER --> LOG["audit.log"]
```

技术栈：

- Tauri 2、Rust、Rayon、WalkDir、Trash
- React 18、TypeScript、Vite
- Tauri Channel 流式扫描进度
- 原生 macOS 废纸篓 API、TCC/FDA 检测

### 项目结构

```text
.
├── src/                       # React 界面
│   ├── components/            # 侧边栏、结果列表、确认框、图标
│   ├── pages/                 # 智能扫描、大文件、规划占位页
│   ├── hooks/                 # 大文件扫描 hook
│   ├── lib/                   # 格式化工具
│   ├── types.ts               # 前后端共享类型
│   └── App.tsx                # App Shell 与页面切换
├── src-tauri/
│   ├── src/
│   │   ├── safety.rs          # 路径白名单与删除守卫
│   │   ├── cleaner.rs         # 废纸篓、审计日志、永久删除令牌
│   │   ├── scan_manager.rs    # 扫描任务注册与取消
│   │   ├── permissions.rs     # 完全磁盘访问权限检测
│   │   ├── fs_util.rs         # 磁盘占用计算
│   │   ├── scanner/
│   │   │   ├── cache.rs       # 缓存扫描
│   │   │   └── large_files.rs # 大文件/旧文件/旧安装包
│   │   └── main.rs            # Tauri 命令入口
│   ├── capabilities/          # Tauri 权限能力
│   ├── icons/                 # 应用图标
│   ├── Cargo.toml
│   └── tauri.conf.json
├── package.json
└── vite.config.ts
```

### 快速开始

环境要求：

- macOS 12+
- Node.js 20+
- Rust stable
- Xcode Command Line Tools（本项目已在仅安装 CLT 的环境完成构建和打包）

```bash
# 安装前端依赖
npm install

# 开发模式
npm run tauri dev

# 前端类型检查与构建
npm run build

# Rust 单元测试
cd src-tauri && cargo test

# 构建 macOS App 与 DMG
npm run tauri build
```

构建产物：

```text
src-tauri/target/release/bundle/macos/SpaceSweeper.app
src-tauri/target/release/bundle/dmg/SpaceSweeper_0.1.0_aarch64.dmg
```

如果所在网络访问 crates.io 较慢，可以自行配置 Cargo 镜像。项目中的 `src-tauri/.cargo/` 已被 `.gitignore` 忽略，不会把个人镜像配置提交到仓库。

### 使用方式

1. 打开 SpaceSweeper；
2. 选择「智能扫描」或「大文件」；
3. 查看完整路径、大小和判定理由，取消勾选不想删除的条目；
4. 点击「移入废纸篓」并确认；
5. 在废纸篓中确认无误后清空，空间才会真正释放。

完全磁盘访问权限不是默认用户目录扫描的必需项；授予后可以扫描更多受 TCC 保护的目录。授权后建议重启 App。

### 已知限制

- 当前预构建产物为 Apple Silicon `arm64`，源码可在 Intel Mac 上重新构建；
- P0 只扫描固定用户目录，不支持自定义目录或外接卷；
- 大文件/旧文件默认只展示 Top 50；
- 旧文件年龄基于修改时间；macOS APFS 的访问时间不可靠，因此不依赖 atime；
- 空间分析、重复文件、卸载器尚未实现；
- 永久删除 UI 未开放；
- 当前构建为 ad-hoc 签名，未做 Developer ID 公证；分发给其他用户前需要正式签名和公证。

### Roadmap

- **P1**：空间分析树图、自定义目录/外接卷授权、A 类缓存路径扩展；
- **P2**：重复文件三段式检测（大小分组 → 4 KB 哈希 → 完整 BLAKE3），每组至少保留一份；
- **P3**：应用卸载器与残留匹配；
- **发布**：Developer ID 签名、公证、通用二进制构建。

### 参与贡献

请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。所有涉及删除、路径校验、权限处理的改动都必须补充测试，并遵守现有的安全不变量。

### 安全

请阅读 [SECURITY.md](SECURITY.md)。不要在公开 Issue 中披露可被利用的路径逃逸、越权删除或权限绕过问题。

### 许可证

[Apache License 2.0](LICENSE) © 2026 Dingyuan Liu

应用图标为项目资产；复用代码时请替换为自己的图标。

### 致谢

- [Tauri](https://tauri.app/)
- [Rust](https://www.rust-lang.org/)
- [React](https://react.dev/)
- [Vite](https://vite.dev/)
- [Rayon](https://github.com/rayon-rs/rayon)
- [WalkDir](https://github.com/BurntSushi/walkdir)
- [trash-rs](https://github.com/Byron/trash-rs)

### 免责声明

本工具会移动文件到废纸篓。尽管内置了多层安全守卫，仍建议先在测试目录验证，再处理真实数据。作者不对因使用本工具造成的数据丢失承担责任。

---

## English

SpaceSweeper is a macOS disk space discovery and safe reclamation tool built with **Tauri 2, Rust and React**. It focuses on transparency and reversibility rather than aggressive one-click cleaning:

- Every result exposes its full path, size and rationale.
- Deletions go to the macOS Trash by default and can be restored with “Put Back”.
- Git worktrees, symlinks, system paths and the scan roots themselves are rejected by the backend.
- No malware scanning, performance tuning, startup-item management, telemetry or background scanning.

The current release supports cache scanning under `~/Library/Caches` and discovery of large files, old files and old installers. A space treemap, duplicate finder and uninstaller are planned. See the Chinese sections above for the safety model, architecture, build commands and known limitations.

Licensed under the [Apache License 2.0](LICENSE).
