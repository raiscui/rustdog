//! 把构建时的 git 提交号编进二进制。
//!
//! 读数要能回答"这份数是哪个构建量的": `rdog.bulk-path.v1` 的两半各带自己的 `build`, 两个 sha 不
//! 相同就说明两端不是同一次构建, 那条读数作废。CI 里可以用 `GIT_SHA` 显式覆盖 (例如浅克隆没有
//! `.git` 时), 本地构建则退回 `git rev-parse`。
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // HEAD 与 refs 变了才需要重编: 只看 HEAD 这一行会漏掉"同一分支被 amend"这种情况。
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-env-changed=GIT_SHA");

    let from_env = std::env::var("GIT_SHA").ok().filter(|sha| !sha.is_empty());
    let sha = from_env.or_else(git_head).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=GIT_SHA={sha}");
}

fn git_head() -> Option<String> {
    let output = Command::new("git")
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
