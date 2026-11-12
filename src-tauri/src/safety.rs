//! 删除安全守卫：所有进入 cleaner 的路径都必须先经过这里。
//!
//! 判定顺序（任何一步失败都会拒绝，且拒绝原因会展示给用户）：
//! 1. 绝对路径、无 `..`、无 `.git` 组件；
//! 2. 目标存在，且目标本身不是符号链接；
//! 3. canonicalize 之后仍严格位于白名单根目录内部，不能等于根目录本身。
//!
//! 为什么必须 canonicalize：
//! 仅做字符串 `starts_with` 会被 `~/Library/Caches/link -> ~/Documents` 这类符号链接绕过。
//! canonicalize 会把父级链接解析成真实路径，再与同样解析过的白名单根比较。
//!
//! 为什么排除 `.git`：
//! 用户源代码目录位于 Documents / Desktop 等允许根目录内，但版本库是用户资产，
//! 即使单个文件在"大文件"列表里被选中，也绝不允许删除版本库内的任何内容。
//!
//! 为什么"发现"范围不等于"删除"范围：
//! 扫描器只会枚举明确的缓存/日志路径；而白名单额外包含 Downloads / Documents 等，
//! 是为了让 B 类（用户选定的大文件、旧文件）可以被回收。任何未列入白名单的路径，
//! 即使由前端伪造传入，也会在这里被拒绝。

use serde::Serialize;
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// safe：默认勾选，删除后应用会自动重建；
/// ask：默认不勾选，必须先向用户解释后果并获得显式授权。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SafetyTier {
    Safe,
    Ask,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    NotAbsolute,
    ParentTraversal,
    GitRepository,
    Symlink,
    Missing,
    RootItself,
    AskNotConfirmed,
    OutsideWhitelist,
    ForbiddenLocation,
    NotApplicationBundle,
    NotAppResidual,
    ProtectedApplication,
}

impl fmt::Display for RejectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotAbsolute => "不是绝对路径",
            Self::ParentTraversal => "路径包含 ..，已拒绝",
            Self::GitRepository => "路径位于 .git 版本库中，绝不删除",
            Self::Symlink => "符号链接不做删除，避免越过白名单",
            Self::Missing => "目标不存在或无权访问",
            Self::RootItself => "不允许删除白名单根目录本身",
            Self::AskNotConfirmed => "该路径属于需确认档，未获得显式授权",
            Self::OutsideWhitelist => "路径不在删除白名单内",
            Self::ForbiddenLocation => "系统保护路径，绝不删除",
            Self::NotApplicationBundle => "不是可卸载的 .app 应用包",
            Self::NotAppResidual => "不是该应用的可识别残留路径",
            Self::ProtectedApplication => "系统应用或受保护应用，绝不卸载",
        };
        f.write_str(message)
    }
}

impl std::error::Error for RejectReason {}

#[derive(Debug, Clone)]
pub struct SafetyPolicy {
    safe_roots: Vec<PathBuf>,
    ask_roots: Vec<PathBuf>,
}

impl SafetyPolicy {
    pub fn new(safe_roots: Vec<PathBuf>, ask_roots: Vec<PathBuf>) -> Self {
        Self {
            safe_roots: safe_roots.into_iter().filter_map(resolve_root).collect(),
            ask_roots: ask_roots.into_iter().filter_map(resolve_root).collect(),
        }
    }

    /// 会话级授权：只追加到当前进程内的安全根，不写入磁盘，重启后自动失效。
    pub fn add_safe_root(&mut self, root: PathBuf) -> Result<PathBuf, String> {
        let resolved = resolve_root(root).ok_or_else(|| "该目录不能作为删除授权根".to_string())?;
        if !self.safe_roots.contains(&resolved) {
            self.safe_roots.push(resolved.clone());
        }
        Ok(resolved)
    }

    pub fn safe_roots(&self) -> &[PathBuf] {
        &self.safe_roots
    }

    pub fn ask_roots(&self) -> &[PathBuf] {
        &self.ask_roots
    }

    /// 生产环境的白名单。
    ///
    /// 只列出"可再生垃圾"和"用户主动选定要清理的个人目录"。
    /// `~/Library/Application Support`、`~/Library/Containers` 等包含个人数据的目录
    /// 不在此列（MobileSync 备份是唯一经 ask 档单独授权的例外）。
    pub fn macos_default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));

        let safe_roots = vec![
            // ---- A 类：可再生垃圾 ----
            home.join("Library/Caches"),
            home.join("Library/Logs"),
            home.join("Library/Developer/Xcode/DerivedData"),
            home.join("Library/Developer/CoreSimulator/Caches"),
            home.join(".npm"),
            home.join(".gradle/caches"),
            home.join(".cache/pip"),
            home.join(".cargo/registry/cache"),
            // ---- B 类：用户主动选择的个人目录（默认扫描目标）----
            // 这些目录里可能有源代码；.git 检查会在下面兜底拒绝版本库内容。
            home.join("Downloads"),
            home.join("Desktop"),
            home.join("Documents"),
            home.join("Movies"),
        ];

        let ask_roots = vec![
            // 设备符号：删除后需要重新连接设备才能再生，部分旧系统版本无法再生。
            home.join("Library/Developer/Xcode/iOS DeviceSupport"),
            // Docker.raw：删除等于丢失全部容器、镜像与卷。
            home.join("Library/Containers/com.docker.docker"),
            // 模拟器设备状态。
            home.join("Library/Developer/CoreSimulator/Devices"),
            // 大型模型缓存：删除后需要重新下载。
            home.join(".ollama/models"),
            home.join(".cache/huggingface"),
            // iOS 备份：属于用户数据，必须逐项确认。
            home.join("Library/Application Support/MobileSync/Backup"),
        ];

        Self::new(safe_roots, ask_roots)
    }

    /// 返回路径所属档位；被拒绝时给出可展示的中文原因。
    pub fn classify(&self, path: &Path) -> Result<SafetyTier, RejectReason> {
        check_lexical(path)?;

        // symlink_metadata 不跟随目标本身，用于识别"待删对象是链接"。
        let meta = path.symlink_metadata().map_err(|_| RejectReason::Missing)?;
        if meta.file_type().is_symlink() {
            return Err(RejectReason::Symlink);
        }

        // 解析父级符号链接后再做白名单判断，防止 link -> 外部目录。
        let canonical = path.canonicalize().map_err(|_| RejectReason::Missing)?;

        if has_git_component(&canonical) {
            return Err(RejectReason::GitRepository);
        }
        if is_forbidden_location(&canonical) {
            return Err(RejectReason::ForbiddenLocation);
        }

        let mut root_hit = false;

        // safe 优先：即使 ask 根目录将来嵌套在 safe 根目录内，也不会被误判为 ask。
        for root in &self.safe_roots {
            if canonical.as_path() == root.as_path() {
                root_hit = true;
            } else if canonical.starts_with(root) {
                return Ok(SafetyTier::Safe);
            }
        }

        for root in &self.ask_roots {
            if canonical.as_path() == root.as_path() {
                root_hit = true;
            } else if canonical.starts_with(root) {
                return Ok(SafetyTier::Ask);
            }
        }

        if root_hit {
            Err(RejectReason::RootItself)
        } else {
            Err(RejectReason::OutsideWhitelist)
        }
    }

    /// 删除前的最终闸门。ask 档必须由调用方显式传入 `allow_ask`。
    ///
    /// 这里比 `classify` 多做一次"目录内是否含版本库"的递归检查：
    /// 扫描阶段允许展示 `~/Documents/projects`，但若其中任何一层存在 `.git`，
    /// 删除整个目录就会连带毁掉源代码版本库，因此必须拒绝。
    /// 该检查只在真正删除时执行，不影响扫描性能。
    pub fn validate_for_deletion(
        &self,
        path: &Path,
        allow_ask: bool,
    ) -> Result<SafetyTier, RejectReason> {
        let tier = self.classify(path)?;
        if tier == SafetyTier::Ask && !allow_ask {
            return Err(RejectReason::AskNotConfirmed);
        }
        // 文件可能位于某个仓库的工作树内，路径本身不含 `.git` 组件；
        // 向上检查祖先目录，确保仓库里的任何文件都不会被删除。
        if is_inside_git_worktree(path) {
            return Err(RejectReason::GitRepository);
        }
        if path.is_dir() && contains_git_repository(path) {
            return Err(RejectReason::GitRepository);
        }
        Ok(tier)
    }

    /// 卸载器专用：只允许 `/Applications` 或 `~/Applications` 下的 `.app` 包。
    pub fn validate_application(path: &Path, bundle_id: Option<&str>) -> Result<(), RejectReason> {
        let mut roots = vec![PathBuf::from("/Applications")];
        if let Some(home) = dirs::home_dir() {
            roots.push(home.join("Applications"));
        }
        let current = current_app_bundle();
        validate_application_in(path, bundle_id, &roots, current.as_deref())
    }

    /// 卸载器专用：校验 Library 内的应用残留是否属于该 Bundle ID / 应用名。
    pub fn validate_app_residual(
        &self,
        path: &Path,
        bundle_id: &str,
        app_name: &str,
        allow_ask: bool,
    ) -> Result<SafetyTier, RejectReason> {
        let home = dirs::home_dir().ok_or(RejectReason::Missing)?;
        validate_app_residual_in(path, bundle_id, app_name, allow_ask, &home)
    }
}

/// 规范约定的入口：生产代码请用 `SafetyPolicy` 复用白名单，此函数便于单点断言。
#[allow(dead_code)] // 规范要求的公共守卫入口；单测会直接调用。
pub fn is_safe_path(path: &Path) -> bool {
    SafetyPolicy::macos_default()
        .validate_for_deletion(path, false)
        .is_ok()
}

/// 空间分析是只读操作，允许主目录（含 Library）与 `/Volumes` 下的具体目录，
/// 但不允许系统根目录或整个卷根。删除权限仍由 `SafetyPolicy` 单独决定。
pub fn is_analysis_root_allowed(path: &Path) -> bool {
    if !path.is_absolute() || path == Path::new("/") || is_forbidden_location(path) {
        // 主目录本身被 is_forbidden_location 拒绝，但作为只读分析根是允许的。
        return dirs::home_dir().map(|home| path == home).unwrap_or(false);
    }

    if let Some(home) = dirs::home_dir() {
        if path == home || path.starts_with(&home) {
            return true;
        }
    }

    path.starts_with(Path::new("/Volumes")) && path != Path::new("/Volumes")
}

fn validate_application_in(
    path: &Path,
    bundle_id: Option<&str>,
    roots: &[PathBuf],
    current_app: Option<&Path>,
) -> Result<(), RejectReason> {
    check_lexical(path)?;

    let metadata = path.symlink_metadata().map_err(|_| RejectReason::Missing)?;
    if metadata.file_type().is_symlink() {
        return Err(RejectReason::Symlink);
    }
    if !metadata.is_dir() || !is_app_bundle_name(path) {
        return Err(RejectReason::NotApplicationBundle);
    }

    let canonical = path.canonicalize().map_err(|_| RejectReason::Missing)?;
    if has_git_component(&canonical) {
        return Err(RejectReason::GitRepository);
    }
    if is_forbidden_location(&canonical) {
        return Err(RejectReason::ForbiddenLocation);
    }
    if bundle_id
        .map(|id| id.to_ascii_lowercase().starts_with("com.apple."))
        .unwrap_or(false)
    {
        return Err(RejectReason::ProtectedApplication);
    }
    if current_app
        .map(|current| current == canonical)
        .unwrap_or(false)
    {
        return Err(RejectReason::ProtectedApplication);
    }

    for root in roots {
        let Ok(resolved_root) = root.canonicalize() else {
            continue;
        };
        if canonical == resolved_root || !canonical.starts_with(&resolved_root) {
            continue;
        }
        if !has_nested_app_ancestor(&canonical, &resolved_root) {
            return Ok(());
        }
    }

    Err(RejectReason::NotApplicationBundle)
}

const APP_RESIDUAL_ROOTS: &[(&str, SafetyTier)] = &[
    ("Library/Caches", SafetyTier::Safe),
    ("Library/Logs", SafetyTier::Safe),
    ("Library/HTTPStorages", SafetyTier::Safe),
    ("Library/WebKit", SafetyTier::Safe),
    ("Library/Cookies", SafetyTier::Safe),
    ("Library/Application Support", SafetyTier::Ask),
    ("Library/Containers", SafetyTier::Ask),
    ("Library/Group Containers", SafetyTier::Ask),
    ("Library/Application Scripts", SafetyTier::Ask),
    ("Library/Preferences", SafetyTier::Ask),
    ("Library/Preferences/ByHost", SafetyTier::Ask),
    ("Library/Saved Application State", SafetyTier::Ask),
    ("Library/LaunchAgents", SafetyTier::Ask),
];

fn validate_app_residual_in(
    path: &Path,
    bundle_id: &str,
    app_name: &str,
    allow_ask: bool,
    home: &Path,
) -> Result<SafetyTier, RejectReason> {
    check_lexical(path)?;

    let metadata = path.symlink_metadata().map_err(|_| RejectReason::Missing)?;
    if metadata.file_type().is_symlink() {
        return Err(RejectReason::Symlink);
    }
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(RejectReason::NotAppResidual);
    }

    let canonical = path.canonicalize().map_err(|_| RejectReason::Missing)?;
    if has_git_component(&canonical) {
        return Err(RejectReason::GitRepository);
    }
    if is_inside_git_worktree(&canonical) {
        return Err(RejectReason::GitRepository);
    }

    let Some(name) = canonical.file_name().map(|name| name.to_string_lossy()) else {
        return Err(RejectReason::NotAppResidual);
    };

    let mut matched: Option<SafetyTier> = None;
    let mut matched_depth = 0;

    for (relative, tier) in APP_RESIDUAL_ROOTS {
        let root = home.join(relative);
        let Ok(root) = root.canonicalize() else {
            continue;
        };
        if canonical == root || canonical.parent() != Some(root.as_path()) {
            continue;
        }
        if !residual_name_matches(&name, bundle_id, app_name, relative) {
            continue;
        }
        let depth = root.components().count();
        if depth >= matched_depth {
            matched = Some(*tier);
            matched_depth = depth;
        }
    }

    let tier = matched.ok_or(RejectReason::NotAppResidual)?;
    if tier == SafetyTier::Ask && !allow_ask {
        return Err(RejectReason::AskNotConfirmed);
    }
    Ok(tier)
}

fn residual_name_matches(name: &str, bundle_id: &str, app_name: &str, relative: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower
        .strip_suffix(".plist")
        .or_else(|| lower.strip_suffix(".savedstate"))
        .or_else(|| lower.strip_suffix(".binarycookies"))
        .unwrap_or(&lower);
    let bundle = bundle_id.trim().to_ascii_lowercase();
    let app = app_name.trim().to_ascii_lowercase();

    let suffix_allowed = relative.ends_with("ByHost")
        || relative.ends_with("Saved Application State")
        || relative.ends_with("Cookies")
        || relative.ends_with("LaunchAgents");

    let bundle_match = !bundle.is_empty()
        && (stem == bundle
            || (suffix_allowed && stem.starts_with(&format!("{bundle}.")))
            || (relative.contains("Group Containers") && lower.contains(&bundle)));
    let app_match = !app.is_empty() && stem == app;
    bundle_match || app_match
}

fn is_app_bundle_name(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext.to_string_lossy().eq_ignore_ascii_case("app"))
        .unwrap_or(false)
}

fn has_nested_app_ancestor(path: &Path, root: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    let components: Vec<&Path> = relative.ancestors().collect();
    components
        .iter()
        .skip(1)
        .any(|component| component != &Path::new("") && is_app_bundle_name(component))
}

fn current_app_bundle() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    executable
        .ancestors()
        .find(|path| is_app_bundle_name(path))
        .and_then(|path| path.canonicalize().ok())
}

/// 校验用户主动选择的自定义目录能否成为会话级删除授权根。
///
/// 只允许主目录内（排除 `Library` 与隐藏目录）或 `/Volumes` 外接卷内的目录；
/// 主目录本身、卷根、系统保护路径、符号链接一律拒绝。
pub fn authorize_session_root(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("请选择绝对路径".to_string());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("路径包含 ..，已拒绝".to_string());
    }

    let metadata = path
        .symlink_metadata()
        .map_err(|_| "所选目录不存在或无权访问".to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("不授权符号链接目录".to_string());
    }
    if !metadata.is_dir() {
        return Err("请选择目录，而不是文件".to_string());
    }

    let canonical = path
        .canonicalize()
        .map_err(|_| "无法解析所选目录".to_string())?;
    let home = dirs::home_dir();
    validate_session_root(&canonical, home.as_deref(), Path::new("/Volumes"))?;
    Ok(canonical)
}

fn validate_session_root(
    canonical: &Path,
    home: Option<&Path>,
    volumes: &Path,
) -> Result<(), String> {
    if canonical == Path::new("/") || is_forbidden_location(canonical) {
        return Err("系统保护路径不能授权".to_string());
    }

    if let Some(home) = home {
        if canonical == home {
            return Err("不能授权主目录本身，请选择具体子目录".to_string());
        }
        if let Ok(relative) = canonical.strip_prefix(home) {
            let mut components = relative.components();
            match components.next() {
                Some(Component::Normal(name)) => {
                    let name = name.to_string_lossy();
                    if name.eq_ignore_ascii_case("Library") {
                        return Err("Library 目录不开放自定义授权".to_string());
                    }
                    if name.starts_with('.') {
                        return Err("隐藏目录不开放自定义授权".to_string());
                    }
                    return Ok(());
                }
                _ => return Err("不能授权主目录本身".to_string()),
            }
        }
    }

    if canonical == volumes {
        return Err("不能授权整个 /Volumes，请选择具体外接卷或目录".to_string());
    }
    if canonical.starts_with(volumes) {
        return Ok(());
    }

    Err("只允许授权主目录或外接卷内的目录".to_string())
}

fn resolve_root(root: PathBuf) -> Option<PathBuf> {
    // 白名单根本身是符号链接时直接丢弃：否则 `~/Library/Caches -> ~/Library/Application Support`
    // 这类链接会把一个"绝不触碰"目录变成可删除区域。
    if root
        .symlink_metadata()
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return None;
    }

    // 根目录可能尚未创建（例如未安装 Docker）。此时保留原路径；
    // 目标若存在，其 canonical 路径不可能落在一个不存在的根目录下，因此仍然是安全的。
    let resolved = root.canonicalize().unwrap_or(root);

    // 防二次绕过：链接解析后不能是文件系统根、用户主目录或系统保护目录。
    if resolved.as_path() == Path::new("/")
        || is_forbidden_location(&resolved)
        || dirs::home_dir()
            .map(|home| resolved.as_path() == home.as_path())
            .unwrap_or(false)
    {
        return None;
    }

    Some(resolved)
}

fn check_lexical(path: &Path) -> Result<(), RejectReason> {
    if !path.is_absolute() {
        // 相对路径、`~` 开头的路径都在这里被拒绝，避免依赖当前工作目录。
        return Err(RejectReason::NotAbsolute);
    }

    for component in path.components() {
        match component {
            Component::ParentDir => return Err(RejectReason::ParentTraversal),
            Component::Normal(name) if is_git_name(name) => {
                return Err(RejectReason::GitRepository)
            }
            _ => {}
        }
    }

    Ok(())
}

fn has_git_component(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => is_git_name(name),
        _ => false,
    })
}

/// 删除目录前递归检查是否包含 `.git`。不跟随符号链接，避免被链接带到安全守卫之外。
fn contains_git_repository(root: &Path) -> bool {
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        // `.git` 可能是目录，也可能是 worktree / submodule 的 `gitdir:` 文件，两者都要保护。
        if is_git_name(entry.file_name()) {
            return true;
        }
    }
    false
}

/// 从待删路径向上检查祖先目录是否为 Git 工作树根。
/// 这样即使前端只提交了仓库内的单个文件，也会在删除前被拒绝。
fn is_inside_git_worktree(path: &Path) -> bool {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir.join(".git").symlink_metadata().is_ok() {
            return true;
        }
        current = dir.parent();
    }
    false
}

fn is_git_name(name: &std::ffi::OsStr) -> bool {
    // macOS 默认大小写不敏感，用忽略大小写比较避免 `.GIT` 绕过。
    name.to_string_lossy().eq_ignore_ascii_case(".git")
}

fn is_forbidden_location(path: &Path) -> bool {
    if path == Path::new("/") {
        return true;
    }

    if let Some(home) = dirs::home_dir() {
        if path == home.as_path() {
            return true;
        }
    }

    const FORBIDDEN_ROOTS: &[&str] = &[
        "/System",
        "/usr",
        "/bin",
        "/sbin",
        "/etc",
        "/private/etc",
        "/Library",
    ];

    FORBIDDEN_ROOTS
        .iter()
        .any(|root| path.starts_with(Path::new(*root)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    struct Fixture {
        _temp: TempDir,
        policy: SafetyPolicy,
        safe_root: PathBuf,
        ask_root: PathBuf,
    }

    fn fixture() -> Fixture {
        let temp = TempDir::new().unwrap();
        let safe_root = temp.path().join("Library/Caches");
        let ask_root = temp.path().join("Developer/Devices");
        fs::create_dir_all(&safe_root).unwrap();
        fs::create_dir_all(&ask_root).unwrap();

        let policy = SafetyPolicy::new(vec![safe_root.clone()], vec![ask_root.clone()]);

        Fixture {
            _temp: temp,
            policy,
            safe_root,
            ask_root,
        }
    }

    #[test]
    fn accepts_file_strictly_inside_safe_root() {
        let f = fixture();
        let file = f.safe_root.join("app/cache.bin");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"cache").unwrap();

        assert_eq!(f.policy.classify(&file), Ok(SafetyTier::Safe));
        assert_eq!(
            f.policy.validate_for_deletion(&file, false),
            Ok(SafetyTier::Safe)
        );
    }

    #[test]
    fn rejects_root_itself() {
        let f = fixture();
        assert_eq!(
            f.policy.classify(&f.safe_root),
            Err(RejectReason::RootItself)
        );
    }

    #[test]
    fn rejects_parent_traversal_even_if_it_resolves_inside() {
        let f = fixture();
        let path = f.safe_root.join("../Caches/app");
        fs::create_dir_all(&path).unwrap();

        assert_eq!(f.policy.classify(&path), Err(RejectReason::ParentTraversal));
    }

    #[test]
    fn rejects_system_paths() {
        let f = fixture();
        assert_eq!(
            f.policy.classify(Path::new("/System/Library")),
            Err(RejectReason::ForbiddenLocation)
        );
        assert_eq!(
            f.policy.classify(Path::new("/usr/bin")),
            Err(RejectReason::ForbiddenLocation)
        );
    }

    #[test]
    fn rejects_path_outside_whitelist() {
        let f = fixture();
        let outside = f._temp.path().join("Documents/report.pdf");
        fs::create_dir_all(outside.parent().unwrap()).unwrap();
        fs::write(&outside, b"user data").unwrap();

        assert_eq!(
            f.policy.classify(&outside),
            Err(RejectReason::OutsideWhitelist)
        );
    }

    #[test]
    fn default_policy_covers_acceptance_cases() {
        let policy = SafetyPolicy::macos_default();
        let home = dirs::home_dir().unwrap();

        // 白名单根目录本身不可删。
        assert_eq!(
            policy.classify(&home.join("Library/Caches")),
            Err(RejectReason::RootItself)
        );
        // 即使最终解析回 Documents，只要路径文本含 .. 就直接拒绝。
        assert_eq!(
            policy.classify(&home.join("Library/Caches/../../Documents")),
            Err(RejectReason::ParentTraversal)
        );
        // 系统路径不可删。
        assert_eq!(
            policy.classify(Path::new("/System/Library")),
            Err(RejectReason::ForbiddenLocation)
        );
        // 个人数据目录不在白名单内。
        assert_eq!(
            policy.classify(&home.join("Library/Application Support")),
            Err(RejectReason::OutsideWhitelist)
        );
    }

    #[test]
    fn public_is_safe_path_rejects_acceptance_cases() {
        let home = dirs::home_dir().unwrap();

        assert!(!is_safe_path(&home.join("Library/Caches")));
        assert!(!is_safe_path(&home.join("Library/Caches/../../Documents")));
        assert!(!is_safe_path(Path::new("/System/Library")));
        assert!(!is_safe_path(&home.join("Library/Application Support")));
    }

    #[test]
    fn rejects_git_repository_content() {
        let f = fixture();
        let inside_git = f.safe_root.join("repo/.git/config");
        fs::create_dir_all(inside_git.parent().unwrap()).unwrap();
        fs::write(&inside_git, b"[core]").unwrap();

        assert_eq!(
            f.policy.classify(&inside_git),
            Err(RejectReason::GitRepository)
        );
    }

    #[test]
    fn deletion_rejects_directory_containing_nested_git_repository() {
        let f = fixture();
        let project = f.safe_root.join("project");
        let git_dir = project.join("nested/.git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), b"ref: refs/heads/main").unwrap();

        // 扫描阶段可以展示这个目录……
        assert_eq!(f.policy.classify(&project), Ok(SafetyTier::Safe));
        // ……但删除时必须因为包含 .git 而整项拒绝。
        assert_eq!(
            f.policy.validate_for_deletion(&project, false),
            Err(RejectReason::GitRepository)
        );
    }

    #[test]
    fn deletion_rejects_file_inside_git_worktree() {
        let f = fixture();
        let repo = f.safe_root.join("project");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("src/large.bin");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, vec![0u8; 4096]).unwrap();

        // 单看路径本身是白名单内的普通文件……
        assert_eq!(f.policy.classify(&file), Ok(SafetyTier::Safe));
        // ……但祖先目录是 Git 工作树，删除必须拒绝。
        assert_eq!(
            f.policy.validate_for_deletion(&file, false),
            Err(RejectReason::GitRepository)
        );
    }

    #[test]
    fn deletion_rejects_directory_inside_git_worktree() {
        let f = fixture();
        let repo = f.safe_root.join("project");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let subdir = repo.join("src");
        fs::create_dir_all(&subdir).unwrap();

        assert_eq!(
            f.policy.validate_for_deletion(&subdir, false),
            Err(RejectReason::GitRepository)
        );
    }

    #[test]
    fn rejects_symlink_target() {
        let f = fixture();
        let outside = f.safe_root.join("../Documents/secret.txt");
        fs::create_dir_all(outside.parent().unwrap()).unwrap();
        fs::write(&outside, b"secret").unwrap();

        let link = f.safe_root.join("escape");
        symlink(&outside, &link).unwrap();

        assert_eq!(f.policy.classify(&link), Err(RejectReason::Symlink));
    }

    #[test]
    fn rejects_symlinked_parent_that_escapes_root() {
        let f = fixture();
        let outside_dir = f.safe_root.join("../Documents");
        fs::create_dir_all(&outside_dir).unwrap();
        let secret = outside_dir.join("secret.txt");
        fs::write(&secret, b"secret").unwrap();

        let link_dir = f.safe_root.join("linked");
        symlink(&outside_dir, &link_dir).unwrap();

        assert_eq!(
            f.policy.classify(&link_dir.join("secret.txt")),
            Err(RejectReason::OutsideWhitelist)
        );
    }

    #[test]
    fn drops_whitelist_root_that_is_itself_a_symlink() {
        let temp = TempDir::new().unwrap();
        let real_root = temp.path().join("Documents");
        fs::create_dir_all(&real_root).unwrap();
        let file = real_root.join("keep.txt");
        fs::write(&file, b"keep").unwrap();

        let linked_root = temp.path().join("CachesLink");
        symlink(&real_root, &linked_root).unwrap();

        let policy = SafetyPolicy::new(vec![linked_root], vec![]);
        assert_eq!(policy.classify(&file), Err(RejectReason::OutsideWhitelist));
    }

    #[test]
    fn ask_tier_requires_explicit_confirmation() {
        let f = fixture();
        let file = f.ask_root.join("device/symbols.bin");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"symbols").unwrap();

        assert_eq!(
            f.policy.validate_for_deletion(&file, false),
            Err(RejectReason::AskNotConfirmed)
        );
        assert_eq!(f.policy.classify(&file), Ok(SafetyTier::Ask));
        assert_eq!(
            f.policy.validate_for_deletion(&file, true),
            Ok(SafetyTier::Ask)
        );
    }

    #[test]
    fn rejects_relative_path_and_tilde() {
        let f = fixture();
        assert_eq!(
            f.policy.classify(Path::new("Library/Caches/app")),
            Err(RejectReason::NotAbsolute)
        );
        assert_eq!(
            f.policy.classify(Path::new("~/Library/Caches/app")),
            Err(RejectReason::NotAbsolute)
        );
    }

    #[test]
    fn rejects_missing_target() {
        let f = fixture();
        assert_eq!(
            f.policy.classify(&f.safe_root.join("no-such-cache")),
            Err(RejectReason::Missing)
        );
    }

    #[test]
    fn session_root_accepts_home_subdirectory_and_external_volume() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("home");
        let volumes = temp.path().join("Volumes");
        let project = home.join("Projects");
        let external = volumes.join("ExternalDisk");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&external).unwrap();

        assert!(validate_session_root(&project, Some(&home), &volumes).is_ok());
        assert!(validate_session_root(&external, Some(&home), &volumes).is_ok());
    }

    #[test]
    fn session_root_rejects_dangerous_locations() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("home");
        let volumes = temp.path().join("Volumes");
        let project = home.join("Projects");
        let library = home.join("Library");
        let hidden = home.join(".ssh");
        let external = volumes.join("ExternalDisk");
        for path in [&project, &library, &hidden, &external] {
            fs::create_dir_all(path).unwrap();
        }

        assert!(validate_session_root(&home, Some(&home), &volumes).is_err());
        assert!(validate_session_root(&library, Some(&home), &volumes).is_err());
        assert!(validate_session_root(&hidden, Some(&home), &volumes).is_err());
        assert!(validate_session_root(&volumes, Some(&home), &volumes).is_err());
        assert!(validate_session_root(Path::new("/System"), Some(&home), &volumes).is_err());
        assert!(validate_session_root(temp.path(), Some(&home), &volumes).is_err());
    }

    #[test]
    fn session_root_rejects_symlink() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("Projects");
        fs::create_dir_all(&target).unwrap();
        let link = temp.path().join("ProjectsLink");
        symlink(&target, &link).unwrap();

        assert!(authorize_session_root(&link).is_err());
    }

    #[test]
    fn application_validation_accepts_only_top_level_app_bundles() {
        let temp = TempDir::new().unwrap();
        let apps = temp.path().join("Applications");
        let app = apps.join("Foo.app");
        let nested = app.join("Contents/Helpers/Bar.app");
        let outside = temp.path().join("Other/Foo.app");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(&outside).unwrap();

        let roots = vec![apps];
        assert!(validate_application_in(&app, None, &roots, None).is_ok());
        assert_eq!(
            validate_application_in(&nested, None, &roots, None),
            Err(RejectReason::NotApplicationBundle)
        );
        assert_eq!(
            validate_application_in(&outside, None, &roots, None),
            Err(RejectReason::NotApplicationBundle)
        );
        assert_eq!(
            validate_application_in(&app, Some("com.apple.Safari"), &roots, None),
            Err(RejectReason::ProtectedApplication)
        );

        let link = temp.path().join("Linked.app");
        symlink(&app, &link).unwrap();
        assert_eq!(
            validate_application_in(&link, None, &roots, None),
            Err(RejectReason::Symlink)
        );
    }

    #[test]
    fn residual_validation_matches_bundle_id_and_classifies_tiers() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("home");
        let safe = home.join("Library/Caches/com.example.foo");
        let logs_by_name = home.join("Library/Logs/Foo");
        let ask = home.join("Library/Application Support/com.example.foo");
        let prefs = home.join("Library/Preferences/com.example.foo.plist");
        let group = home.join("Library/Group Containers/TEAMID.group.com.example.foo");
        let other = home.join("Library/Caches/other.app");
        for path in [&safe, &logs_by_name, &ask, &prefs, &group, &other] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if path.extension().is_some() {
                fs::write(path, b"plist").unwrap();
            } else {
                fs::create_dir_all(path).unwrap();
            }
        }

        assert_eq!(
            validate_app_residual_in(&safe, "com.example.foo", "Foo", false, &home),
            Ok(SafetyTier::Safe)
        );
        assert_eq!(
            validate_app_residual_in(&logs_by_name, "com.example.foo", "Foo", false, &home),
            Ok(SafetyTier::Safe)
        );
        assert_eq!(
            validate_app_residual_in(&ask, "com.example.foo", "Foo", false, &home),
            Err(RejectReason::AskNotConfirmed)
        );
        assert_eq!(
            validate_app_residual_in(&ask, "com.example.foo", "Foo", true, &home),
            Ok(SafetyTier::Ask)
        );
        assert_eq!(
            validate_app_residual_in(&prefs, "com.example.foo", "Foo", true, &home),
            Ok(SafetyTier::Ask)
        );
        assert_eq!(
            validate_app_residual_in(&group, "com.example.foo", "Foo", true, &home),
            Ok(SafetyTier::Ask)
        );
        assert_eq!(
            validate_app_residual_in(&other, "com.example.foo", "Foo", false, &home),
            Err(RejectReason::NotAppResidual)
        );
        assert_eq!(
            validate_app_residual_in(
                &home.join("Library/Caches"),
                "com.example.foo",
                "Foo",
                false,
                &home
            ),
            Err(RejectReason::NotAppResidual)
        );
    }
}
