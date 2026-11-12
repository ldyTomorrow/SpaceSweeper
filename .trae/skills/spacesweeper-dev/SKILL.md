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
- 版本：`0.1.0`，P0 已完成。
- 分支：`dev` 日常开发；`main` 保持稳定/可发布。
- 远程：`git@github.com:ldyTomrrom/SpaceSweeper.git`。
- 许可证：Apache-2.0。
- P0 基线提交：`b8bfcc0`；最新状态以 `git log` 为准。
- 非目标：病毒查杀、恶意软件检测、性能优化、启动项管理、隐私数据清理、自动清理、遥测、第三方清理软件对比。

## 架构速览

```text
React UI
  └─ Tauri IPC / Channel
       └─ Rust commands (main.rs)
            ├─ scanner/cache.rs        缓存流式扫描
            ├─ scanner/large_files.rs  大文件/旧文件/旧安装包 Top N
            ├─ scan_manager.rs         扫描任务与取消
            ├─ safety.rs               路径白名单与删除守卫
            ├─ cleaner.rs              废纸篓、审计、永久删除令牌
            └─ permissions.rs          FDA 检测
```

关键行为：

- 缓存扫描：逐条 Channel 推送，前端实时渲染。
- 大文件扫描：进度流式推送，结果在结束时按三组 Top 50 返回。
- `SafetyPolicy` 是删除权限的最终裁决者，前端过滤不能替代后端校验。
- `App.tsx` 用 `.page-slot` 保持页面挂载，切换侧边栏时扫描状态不丢失。

## 关键文件索引

| 路径 | 作用 | 何时读取 |
| --- | --- | --- |
| `src/App.tsx` | App Shell、侧边栏页面切换、FDA 状态 | 改导航、全局壳层、页面挂载方式 |
| `src/components/Sidebar.tsx` | 五个模块入口与规划状态 | 改导航结构、模块入口 |
| `src/pages/SmartScanPage.tsx` | 缓存扫描页面、结果选择、删除确认 | 改缓存扫描 UI/流程 |
| `src/pages/BigFilesPage.tsx` | 大文件/旧文件/旧安装包页面 | 改扫描选项、Tabs、删除流程 |
| `src/pages/ComingSoonPage.tsx` | P1/P2/P3 占位页 | 新模块接入前 |
| `src/hooks/useLargeFilesScan.ts` | 大文件扫描 Channel、进度、取消、结果状态 | 改大文件前端状态 |
| `src/components/ScanResultList.tsx` | 缓存结果列表 | 改缓存行、勾选、展开 |
| `src/components/LargeFileList.tsx` | 大文件结果列表与 Finder 定位入口 | 改文件行、详情、定位 |
| `src/components/DeleteConfirmDialog.tsx` | 统一删除确认框 | 改删除文案或确认流程 |
| `src/components/FdaNotice.tsx` | FDA 未授权引导 | 改权限引导 |
| `src/types.ts` | 前后端事件与数据模型 | 新增扫描事件、字段 |
| `src/lib/format.ts` | 字节/日期/年龄/路径格式化 | 展示格式改动 |
| `src/styles.css` | 全部布局与组件样式 | UI/响应式改动 |
| `src-tauri/src/main.rs` | Tauri 命令注册、AppState | 新增命令、命令参数 |
| `src-tauri/src/safety.rs` | 白名单、路径规范化、Git 守卫、测试 | 任何删除/路径规则改动 |
| `src-tauri/src/cleaner.rs` | 废纸篓、审计、运行中应用保护、永久删除令牌 | 删除执行、报告、审计 |
| `src-tauri/src/scan_manager.rs` | 扫描任务注册与取消令牌 | 新增长耗时扫描 |
| `src-tauri/src/scanner/cache.rs` | 缓存扫描与事件 | 缓存路径/流式协议 |
| `src-tauri/src/scanner/large_files.rs` | 大文件/旧文件/旧安装包扫描 | 文件发现算法、Top N、过滤 |
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
| 缓存扫描/安全档 | `scanner/cache.rs`、`SmartScanPage.tsx`、`ScanResultList.tsx`、`types.ts`、`main.rs` |
| 大文件/旧文件/旧安装包 | `scanner/large_files.rs`、`BigFilesPage.tsx`、`LargeFileList.tsx`、`useLargeFilesScan.ts`、`types.ts`、`scan_manager.rs`、`main.rs` |
| 删除/路径安全 | `safety.rs`、`cleaner.rs`、`main.rs` 及同文件测试；必要时 `SECURITY.md` |
| 侧边栏/页面导航 | `App.tsx`、`Sidebar.tsx`、对应 `pages/*`、`styles.css` |
| FDA/权限引导 | `permissions.rs`、`FdaNotice.tsx`、`App.tsx` |
| 新增扫描器 | `scanner/mod.rs`、最接近的现有扫描器、`scan_manager.rs`、`main.rs`、`types.ts` |
| 打包/发布 | `package.json`、`tauri.conf.json`、`Cargo.toml`、README「快速开始」 |
| 文档/许可证 | `README.md`、`CONTRIBUTING.md`、`SECURITY.md`、`LICENSE`、`package.json`、`Cargo.toml` |

读取测试：Rust 单测在对应 `.rs` 文件的 `#[cfg(test)]` 模块；先用 `git grep "#[cfg(test)]"` 或符号名定位，不要整文件通读。

## 安全不变量

1. 所有删除路径必须经过 `SafetyPolicy` 二次校验；不能只靠前端勾选。
2. 默认移入废纸篓；永久删除只有后端令牌能力，当前不开放 UI。
3. 拒绝白名单根目录本身、系统路径、`..`、符号链接目标和非白名单路径。
4. 发现阶段跳过版本库根、隐藏目录、应用包、照片库和常见构建目录。
5. 删除文件前向上检查祖先目录是否为 Git 工作树；删除目录前递归检查 `.git`。
6. 不添加自动清理、后台常驻扫描、遥测、订阅弹窗或第三方工具对比文案。
7. 任何新增路径规则都必须补拒绝用例，并在临时目录测试，不使用真实用户数据。

## 常用命令

```bash
npm install
npm run tauri dev
npm run build
cd src-tauri && cargo test
cd src-tauri && cargo fmt --check
npm run tauri build
```

- `cargo test` 当前基线为 38 项通过。
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

- P1 空间分析：新增 `scanner/analyze.rs`、`pages/SpaceAnalysisPage.tsx`、树图组件；先只允许文件节点删除，目录节点定位。
- P1 自定义目录/外接卷：需要扩展 `SafetyPolicy` 的会话级授权根目录；禁止任意绝对路径直传。
- P2 重复文件：`scanner/duplicates.rs`，大小分组 → 4 KB 哈希 → 完整 BLAKE3；每组保留一份，后端二次校验。
- P3 卸载器：`scanner/uninstaller.rs`，Bundle ID 匹配残留；个人数据残留默认不勾选，系统 App 不显示删除入口。
- A 类扩展：按 README 的缓存路径分批接入，复用 `scanner/cache.rs` 的流式协议。

## 已知陷阱

- Tauri 命令参数和事件字段都是 camelCase；Rust 端用 `#[serde(rename_all = "camelCase")]`，前后端类型必须同步。
- `LargeScanOptions.roots` 只接受固定的目录名称（`Downloads`、`Desktop`、`Documents`、`Movies`），不是任意路径。
- `ScanManager` 同时只允许一个扫描任务；新增扫描要复用取消令牌。
- 大文件结果是扫描结束后的 Top N，不是逐条推送；缓存扫描才是逐条推送。
- 页面不要改回条件渲染，否则切换侧边栏会丢失扫描状态。
- 修改 `safety.rs` 后必须跑路径逃逸、根目录、符号链接、祖先 `.git` 测试。
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
