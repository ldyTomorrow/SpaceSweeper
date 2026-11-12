---
name: spacesweeper-dev
description: Guides SpaceSweeper development with file routing, safety rules, tests and release steps. Use for changes, debugging, packaging or review in this repo. Not for unrelated projects.
---

# SpaceSweeper 开发指南

## 使用方式

- 开始任务先读本文件，不要通读整个仓库。
- 按「任务路由」只读必要文件；先用 `git grep` / Grep 定位符号，再定向 Read。
- 大改动完成后按「Skill 更新协议」更新本文件。
- 本文件面向开发者与 AI 代理；用户文档看 `README.md`，安全报告看 `SECURITY.md`。

## 项目基线

- 产品：独立开发的 macOS 磁盘空间发现与安全回收工具。
- 技术栈：Tauri 2 + Rust + React 18 + TypeScript + Vite。
- 版本：`0.2.0`，P0 + P1/P2/P3 功能已完成，等待用户验收。
- 分支：`dev` 日常开发；`main` 保持稳定/可发布。
- 远程：`git@github.com:ldyTomrrom/SpaceSweeper.git`。
- 许可证：Apache-2.0。
- P0 基线提交：`b8bfcc0`；0.2.0 长任务改动在当前 `dev` 工作区，提交后以 `git log` 为准。
- 非目标：病毒查杀、恶意软件检测、性能优化、启动项管理、隐私数据清理、自动清理、遥测、第三方清理软件对比。

## 架构速览

```text
React UI
  └─ Tauri IPC / Channel
       └─ Rust commands (main.rs)
            ├─ scanner/cache.rs        缓存/日志/开发者/包管理器流式扫描
            ├─ scanner/large_files.rs  大文件/旧文件/旧安装包 Top N
            ├─ scanner/analyze.rs      空间分析目录树
            ├─ scanner/duplicates.rs   重复文件三段式检测与删除复核
            ├─ scanner/uninstaller.rs  应用发现、残留匹配与卸载
            ├─ scanner/rules.rs        共享扫描过滤规则
            ├─ scan_manager.rs         扫描任务与取消
            ├─ safety.rs               路径白名单、应用包与残留守卫
            ├─ cleaner.rs              废纸篓、审计、永久删除令牌
            └─ permissions.rs          FDA 检测
```

关键行为：

- 缓存扫描：逐条 Channel 推送，前端实时渲染；支持 app/logs/developer/packages 四组。
- 大文件/重复文件/应用扫描：进度流式推送，结果在结束时返回；重复文件在结束时返回分组。
- 空间分析：进度流式推送，命令返回树；目录节点不可删除，文件节点由白名单决定。
- `SafetyPolicy` 是删除权限的最终裁决者，前端过滤不能替代后端校验。
- `App.tsx` 用 `.page-slot` 保持页面挂载，切换侧边栏时扫描状态不丢失。

## 关键文件索引

| 路径 | 作用 | 何时读取 |
| --- | --- | --- |
| `src/App.tsx` | App Shell、侧边栏页面切换、FDA 状态 | 改导航、全局壳层、页面挂载方式 |
| `src/components/Sidebar.tsx` | 五个模块入口 | 改导航结构、模块入口 |
| `src/pages/SmartScanPage.tsx` | 缓存分组扫描、需确认面板、删除确认 | 改缓存扫描 UI/流程 |
| `src/pages/BigFilesPage.tsx` | 大文件/旧文件/旧安装包页面 | 改扫描选项、Tabs、删除流程 |
| `src/pages/SpaceAnalysisPage.tsx` | 空间分析树图、面包屑、自定义目录 | 改树图、下钻、文件删除 |
| `src/pages/DuplicatesPage.tsx` | 重复文件分组、保留项、删除 | 改重复文件 UI/删除 |
| `src/pages/UninstallerPage.tsx` | 应用列表、残留选择、卸载 | 改应用/残留 UI |
| `src/hooks/useLargeFilesScan.ts` | 大文件扫描 Channel、进度、取消、结果状态 | 改大文件前端状态 |
| `src/hooks/useSpaceAnalysis.ts` | 空间分析 Channel、树、授权、裁剪 | 改空间分析状态 |
| `src/hooks/useDuplicateScan.ts` | 重复文件扫描与删除后状态 | 改重复文件状态 |
| `src/hooks/useAppScan.ts` | 应用扫描 Channel 与取消 | 改应用扫描状态 |
| `src/components/ScanResultList.tsx` | 缓存/需确认结果列表 | 改缓存行、勾选、展开 |
| `src/components/AttentionPanel.tsx` | 需确认区域独立扫描与删除 | 改 ask 档流程 |
| `src/components/SpaceTreemap.tsx` | 空间树图 SVG 渲染 | 改树图交互/配色 |
| `src/components/LargeFileList.tsx` | 大文件结果列表与 Finder 定位入口 | 改文件行、详情、定位 |
| `src/components/DeleteConfirmDialog.tsx` | 统一删除确认、永久删除、风险确认 | 改删除文案或确认流程 |
| `src/components/FdaNotice.tsx` | FDA 未授权引导 | 改权限引导 |
| `src/types.ts` | 前后端事件与数据模型 | 新增扫描事件、字段 |
| `src/lib/format.ts` | 字节/日期/年龄/路径格式化 | 展示格式改动 |
| `src/lib/treemap.ts` | 二分树图布局 | 树图算法改动 |
| `src/styles.css` | 全部布局与组件样式 | UI/响应式改动 |
| `src-tauri/src/main.rs` | Tauri 命令注册、AppState、会话策略 | 新增命令、命令参数 |
| `src-tauri/src/safety.rs` | 白名单、路径规范化、Git/应用/残留守卫、测试 | 任何删除/路径规则改动 |
| `src-tauri/src/cleaner.rs` | 废纸篓、审计、运行中应用保护、永久删除令牌 | 删除执行、报告、审计 |
| `src-tauri/src/scan_manager.rs` | 扫描任务注册与取消令牌 | 新增长耗时扫描 |
| `src-tauri/src/scanner/cache.rs` | 缓存分组/需确认区域扫描与事件 | 缓存路径/流式协议 |
| `src-tauri/src/scanner/large_files.rs` | 大文件/旧文件/旧安装包扫描 | 文件发现算法、Top N、过滤 |
| `src-tauri/src/scanner/analyze.rs` | 空间分析递归聚合与树 | 目录聚合、深度/子项上限 |
| `src-tauri/src/scanner/duplicates.rs` | 重复检测、保留项与删除复核 | 哈希流水线、保留约束 |
| `src-tauri/src/scanner/uninstaller.rs` | 应用发现、Info.plist、残留、卸载 | 应用/残留规则 |
| `src-tauri/src/scanner/rules.rs` | 共享跳过/包/构建目录规则 | 扫描过滤一致性 |
| `src-tauri/src/scanner/mod.rs` | 扫描器模块导出 | 新增扫描器 |
| `src-tauri/src/permissions.rs` | 完全磁盘访问检测 | 权限逻辑 |
| `src-tauri/src/fs_util.rs` | 磁盘占用计算 | 文件大小/宿主资源 |
| `src-tauri/src/main.rs` + `Cargo.toml` | Tauri 启动与依赖 | 新依赖、命令注册 |
| `src-tauri/tauri.conf.json` | 窗口、构建、签名配置 | 打包、窗口、应用标识 |
| `package.json` / `vite.config.ts` | 前端依赖与构建 | 前端依赖、构建配置 |
| `README.md` / `CONTRIBUTING.md` / `SECURITY.md` | 用户与贡献者文档 | 行为、安全、发布说明变化 |
| `LICENSE` | Apache-2.0 | 许可证相关 |

默认不要读取：

- `node_modules/`、`dist/`、`release/`
- `src-tauri/target/`、`src-tauri/gen/`、`src-tauri/.cargo/`
- `package-lock.json`、`src-tauri/Cargo.lock`（除非排查依赖版本/锁定问题）
- `src-tauri/icons/` 下生成的各尺寸图标
- `.git/` 内部对象

## 任务路由

| 任务类型 | 优先读取 |
| --- | --- |
| 缓存扫描/安全档 | `scanner/cache.rs`、`SmartScanPage.tsx`、`AttentionPanel.tsx`、`ScanResultList.tsx`、`types.ts`、`main.rs` |
| 大文件/旧文件/旧安装包 | `scanner/large_files.rs`、`BigFilesPage.tsx`、`LargeFileList.tsx`、`useLargeFilesScan.ts`、`types.ts`、`scan_manager.rs`、`main.rs` |
| 空间分析/自定义目录 | `scanner/analyze.rs`、`SpaceAnalysisPage.tsx`、`SpaceTreemap.tsx`、`useSpaceAnalysis.ts`、`safety.rs`、`main.rs` |
| 重复文件 | `scanner/duplicates.rs`、`DuplicatesPage.tsx`、`useDuplicateScan.ts`、`types.ts`、`cleaner.rs`、`main.rs` |
| 卸载器 | `scanner/uninstaller.rs`、`UninstallerPage.tsx`、`useAppScan.ts`、`safety.rs`、`cleaner.rs`、`main.rs` |
| 删除/路径安全 | `safety.rs`、`cleaner.rs`、`main.rs` 及同文件测试；必要时 `SECURITY.md` |
| 侧边栏/页面导航 | `App.tsx`、`Sidebar.tsx`、对应 `pages/*`、`styles.css` |
| FDA/权限引导 | `permissions.rs`、`FdaNotice.tsx`、`App.tsx` |
| 新增扫描器 | `scanner/mod.rs`、最接近的现有扫描器、`scan_manager.rs`、`main.rs`、`types.ts` |
| 打包/发布 | `package.json`、`tauri.conf.json`、`Cargo.toml`、README「快速开始」 |
| 文档/许可证 | `README.md`、`CONTRIBUTING.md`、`SECURITY.md`、`LICENSE`、`PROJECT_BACKLOG.md`、`package.json`、`Cargo.toml` |

读取测试：Rust 单测在对应 `.rs` 文件的 `#[cfg(test)]` 模块；先用 `git grep "#[cfg(test)]"` 或符号名定位，不要整文件通读。

## 安全不变量

1. 所有删除路径必须经过 `SafetyPolicy` 二次校验；不能只靠前端勾选。
2. 默认移入废纸篓；永久删除必须显式勾选并输入「永久删除」，后端一次性令牌与路径清单绑定。
3. 拒绝白名单根目录本身、系统路径、`..`、符号链接目标和非白名单路径。
4. 发现阶段跳过版本库根、隐藏目录、应用包、照片库和常见构建目录。
5. 删除文件前向上检查祖先目录是否为 Git 工作树；删除目录前递归检查 `.git`。
6. 需确认区域默认不勾选；删除时必须逐项选择并勾选风险确认。
7. 自定义目录授权只允许主目录非 Library 子目录或 `/Volumes` 子目录，且只存在当前进程。
8. 重复文件每组至少保留一份；删除前后端复核保留项存在、大小与完整哈希一致。
9. 卸载器只允许 `/Applications` 与 `~/Applications` 下的 `.app`；以 Info.plist 真实 Bundle ID 为准；系统应用拒绝。
10. 应用残留只允许固定 Library 子目录的直接子项，并匹配 Bundle ID 或应用名。
11. 不添加自动清理、后台常驻扫描、遥测、订阅弹窗或第三方工具对比文案。
12. 任何新增路径规则都必须补拒绝用例，并在临时目录测试，不使用真实用户数据。

## 常用命令

```bash
npm install
npm run tauri dev
npm run build
cd src-tauri && cargo test
cd src-tauri && cargo fmt --check
npm run tauri build
```

- `cargo test` 当前基线为 54 项通过（0.2.0）。
- `npm run tauri build` 生成 `.app` 与 DMG；当前为 ad-hoc 签名，未公证。
- `src-tauri/.cargo/config.toml` 是本地镜像配置，已被忽略；不要提交。
- `release/` 是本地产物目录，不进入 Git。

## Git 与协作

- 开发在 `dev`：`git switch dev` → 修改 → 测试 → commit → `git push`。
- `main` 保持稳定；功能完成后通过 PR 从 `dev` 合并到 `main`。
- 不要强制推送 `main`，不要提交 `release/`、`target/`、`node_modules/`、`.cargo/` 或私钥。
- 提交信息使用 Conventional Commits，例如 `feat(scanner): ...`、`fix(safety): ...`。
- 许可证为 Apache-2.0；新增源文件沿用现有版权与许可约定。
- 不重新引入任何第三方清理软件的名称、对比或“复刻”表述；产品定位是独立原创工具。

## Token 纪律

- 先用 `git status -sb`、`git diff --stat`、`git grep` 定位，再读文件。
- 只读任务路由中列出的文件；大文件用 `Read` 的 offset/limit 读取相关片段。
- 不读锁文件、构建产物、生成图标和第三方依赖源码。
- 修改前后对照测试与调用方，不要为了“确认”而全文读取所有模块。

## 后续模块入口

- 发布：Developer ID 签名、公证、`universal-apple-darwin` 构建；当前仍为 ad-hoc、arm64。
- 体验：空间分析树图缩放/搜索、重复文件按目录筛选、卸载器残留规则扩充。
- 工程：GitHub Actions 运行 `cargo test`、`cargo fmt --check`、`npm run build`。
- A 类扩展：按分组继续接入更多缓存路径，复用 `scanner/cache.rs` 的流式协议。

## 已知陷阱

- Tauri 命令参数和事件字段都是 camelCase；Rust 端用 `#[serde(rename_all = "camelCase")]`，前后端类型必须同步。
- `LargeScanOptions.roots`、`DuplicateScanOptions.roots` 只接受固定目录名称（`Downloads`、`Desktop`、`Documents`、`Movies`），不是任意路径。
- 空间分析的 `root` 是绝对路径，只允许主目录或 `/Volumes` 子目录；自定义目录删除要先生成会话授权。
- `ScanManager` 同时只允许一个扫描任务；空间分析、重复文件、应用扫描共用它，缓存扫描/需确认扫描不占用。
- 大文件/重复文件/应用扫描结果是扫描结束后的汇总；缓存扫描才是逐条推送。
- 页面不要改回条件渲染，否则切换侧边栏会丢失扫描状态。
- 修改 `safety.rs` 后必须跑路径逃逸、根目录、符号链接、祖先 `.git`、应用包与残留匹配测试。
- 卸载器以 Info.plist 的真实 Bundle ID 为准，前端传入的 Bundle ID 只用于一致性校验，不能用于绕过系统应用保护。
- 重复文件删除会重新计算完整 BLAKE3；不要在 UI 层自行放宽“每组保留一份”的约束。
- 空间分析目录节点永远不可删除；只有文件节点在 `SafetyPolicy` 放行后可选。
- macOS APFS 的 atime 不可靠；旧文件判断使用 mtime，不要静默改成 atime。
- 当前预构建产物是 arm64，未做 Developer ID 公证。

## Skill 更新协议

以下变更视为大变更，完成后必须更新本文件：

- 新增/删除扫描模块、Tauri 命令、事件字段或安全规则；
- 修改删除流程、权限策略、白名单或打包方式；
- 新增页面/导航、P1/P2/P3 模块或发布流程；
- 依赖、构建命令、分支/发布策略发生变化。

更新清单：

1. 更新「项目基线」的版本、状态和 P0 基线提交；
2. 更新「关键文件索引」和「任务路由」；
3. 更新「常用命令」「已知陷阱」和「后续模块入口」；
4. 若影响用户行为或安全，同步更新 `README.md`、`CONTRIBUTING.md`、`SECURITY.md`；
5. 运行 `cargo test`、`cargo fmt --check`、`npm run build`；
6. 将本文件与代码一起提交到 `dev` 并推送。

保持本文件低于 500 行；长文档放 `references/` 并按需链接，不要复制整份代码。

最后更新：2026-11-12

P0 基线：`b8bfcc0`

0.2.0 基线提交：`ac58655`
