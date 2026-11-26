//! 把构建时的 git 提交号编进二进制。
//!
//! 读数要能回答"这份数是哪个构建量的": `rdog.bulk-path.v1` 的两半各带自己的 `build`, 两个 sha 不
//! 相同就说明两端不是同一次构建, 那条读数作废。CI 里可以用 `GIT_SHA` 显式覆盖 (例如浅克隆没有
//! `.git` 时), 本地构建则退回 `git rev-parse`。
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // `.git/HEAD` 的内容是 `ref: refs/heads/x`, **提交不会改它** —— 只盯它会烘出一个陈旧的 sha,
    // 而一个贴错标签的读数比 `unknown` 更坏。真正的变更信号是 reflog: 每次 commit / checkout /
    // reset 都会追加一行, mtime 必变。
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/logs/HEAD");
    println!("cargo:rerun-if-env-changed=GIT_SHA");

    let from_env = std::env::var("GIT_SHA").ok().filter(|sha| !sha.is_empty());
    let sha = from_env.or_else(workspace_head).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=GIT_SHA={sha}");
}

/// 只认**本 workspace 根**的 git 元数据: `CARGO_MANIFEST_DIR` 下没有 `.git` 就写 `unknown`。
///
/// 早先直接用 `git rev-parse` 会向上找 —— 把源码包解到别的 checkout 里构建时, 烘进去的是**外层
/// 仓库**的 sha。那正是这一格要消灭的错: 一个贴错标签的读数比 `unknown` 更坏。
fn workspace_head() -> Option<String> {
    // **运行时**读, 不用 `env!`: 编译期常量会把路径烘进 build-script 二进制, Cargo 在另一个目录
    // 构建时会复用那个旧脚本, 于是又贴成别的仓库的 sha (复核实测过)。
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
    // `.git` 可以是目录 (普通 checkout) 或文件 (worktree / submodule)。
    if !root.join(".git").exists() {
        return None;
    }

    // 继承来的 `GIT_DIR` / `GIT_WORK_TREE` 会把探测指回**外层**仓库: 显式清掉, 只认 `-C root`。
    let output = Command::new("git")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if sha.is_empty() {
        None
    } else {
        Some(sha)
    }
}
