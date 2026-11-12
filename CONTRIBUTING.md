# Contributing to SpaceSweeper

感谢你愿意参与 SpaceSweeper。这个项目处理的是用户真实文件，**安全优先于功能数量**。

## 开始之前

请先阅读 [README.md](README.md) 中的「安全设计」和 [SECURITY.md](SECURITY.md)。任何绕过 `src-tauri/src/safety.rs` 的删除路径都不会被接受。

## 开发环境

- macOS 12+
- Node.js 20+
- Rust stable
- Xcode Command Line Tools

```bash
npm install
npm run tauri dev
```

## 提交前检查

```bash
npm run build
cd src-tauri && cargo test
cargo fmt --check
```

如果修改了 Tauri 配置、图标或权限能力，请额外执行一次打包验证：

```bash
npm run tauri build
```

不要对真实用户数据测试删除逻辑。测试应使用 `tempfile::TempDir` 或专门的测试目录。

## 安全不变量

以下规则属于项目底线，不接受为“方便”而放宽：

1. 所有删除路径必须经过 `SafetyPolicy` 二次校验，不能只依赖前端过滤。
2. 默认使用废纸篓；不要新增静默删除、自动清理或后台扫描。
3. 不提供“一键删除全部扫描结果”的入口。
4. 永久删除只能作为显式 opt-in 存在，并且必须保留后端令牌、二次确认和逐项审计日志。
5. 不允许删除白名单根目录本身、系统路径、`..` 路径、符号链接目标或 Git 工作树内的内容。
6. 发现阶段应跳过版本库、隐藏目录、应用包和构建目录；删除阶段也要做对应的路径检查。
7. 不引入遥测、订阅、营销弹窗或隐私数据读取。

涉及路径校验的修改必须补充拒绝用例，例如根目录、`..`、符号链接、祖先 `.git`、非白名单路径。

## 代码约定

- Rust 扫描器放在 `src-tauri/src/scanner/`，删除和审计放在 `cleaner.rs`，路径安全放在 `safety.rs`。
- Tauri 命令在 `src-tauri/src/main.rs` 注册；前端事件字段使用 camelCase。
- 新增扫描任务请复用 `ScanManager` 的取消令牌，并通过 Tauri `Channel` 推送进度。
- 前端页面放在 `src/pages/`，共享组件放在 `src/components/`；沿用现有 CSS 变量和视觉语言。
- 新增文件类型扫描时，默认只读取元数据，不要读取文件内容，除非功能明确需要（例如重复文件哈希）。

## 提交流程

1. 从 `main` 创建功能分支；
2. 保持改动聚焦，一个 PR 只做一件事；
3. Rust 与 TypeScript 均需通过构建和测试；
4. UI 改动请附截图或录屏；
5. PR 描述写清楚：改了什么、为什么、如何验证、是否影响删除安全。

提交信息建议使用 Conventional Commits：

```text
feat(scanner): add old installer discovery
fix(safety): reject files inside git worktrees
docs: clarify full disk access setup
```

## 新增扫描器的最小清单

- 明确扫描根目录和排除规则；
- 复用白名单与 `SafetyPolicy`；
- 支持取消和进度事件；
- 结果包含完整路径、大小、时间和判定理由；
- 提供 Top N 或分页，避免一次性把海量结果推给前端；
- 增加临时目录单测，不依赖真实用户目录；
- 在 README 的「当前功能」和「已知限制」中同步说明。

## 行为准则

保持尊重、具体、对事不对人。安全问题请按 [SECURITY.md](SECURITY.md) 私下报告，不要在公开 Issue 中披露可利用细节。

## 许可证

提交代码即表示你同意以 [Apache License 2.0](LICENSE) 授权你的贡献。
