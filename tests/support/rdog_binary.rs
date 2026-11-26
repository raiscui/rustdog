//! `rdog` 集成测试二进制定位的唯一实现 (RAI-405)。
//!
//! `cargo test` 为集成测试产出的 `rdog` 所在的目录随工具链布局变化:
//! 旧布局在 `target/{profile}/deps/` 的上一级, cargo 1.100 起本 crate 的
//! 测试可执行文件落在 `target/{profile}/build/rustdog/<hash>/out/` (RAI-396)。
//! 各测试文件曾经各自写一份「测试可执行文件上两级 + `/rdog`」的反推,
//! 布局一变就集体假失败; 这里收成一份, 各文件只 `use` 它的结果。
//!
//! 引入方式 (集成测试是独立 crate, 不能共用 lib 侧模块):
//!
//! ```ignore
//! #[path = "support/rdog_binary.rs"]
//! mod rdog_binary;
//! use rdog_binary::rdog_binary_path;
//! ```
//!
//! `tests/` 下含 `support.rs` 的子目录内引用时路径改为 `../support/rdog_binary.rs`。
//!
//! 每个测试 crate 只用到其中一部分条目, 其余的「未使用」不是死代码。
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

/// 被测二进制的文件名 (Windows 带 `.exe`)。
fn executable_name() -> String {
    format!("rdog{}", std::env::consts::EXE_SUFFIX)
}

/// 解析被测的 `rdog` 二进制; 全候选都不存在时 panic 并列出全部尝试过的路径。
pub fn rdog_binary_path() -> PathBuf {
    rdog_binary_path_with_override(&[])
}

/// 同 [`rdog_binary_path`], 但先看 `override_vars` 里的环境变量 —— 真机 e2e 用它们
/// 指向手工部署的二进制。变量存在却指向不存在的文件时直接失败, 不静默回落。
pub fn rdog_binary_path_with_override(override_vars: &[&str]) -> PathBuf {
    for key in override_vars {
        if let Some(path) = std::env::var_os(key) {
            let binary = PathBuf::from(path);
            assert!(
                binary.exists(),
                "{key} 指向的二进制不存在: {}",
                binary.display()
            );
            return binary;
        }
    }

    let current_exe = std::env::current_exe().expect("current test binary path should exist");
    let candidates = rdog_binary_candidates(&current_exe);
    match candidates.iter().find(|path| path.is_file()) {
        Some(binary) => binary.clone(),
        None => {
            let tried = candidates
                .iter()
                .map(|path| format!("\n  - {}", path.display()))
                .collect::<String>();
            panic!(
                "expected the rdog binary beside the test executable ({}); tried:{tried}",
                current_exe.display()
            )
        }
    }
}

/// 按优先级列出 `rdog` 可能所在的位置。
///
/// 首选 cargo 为本包注入的 `CARGO_BIN_EXE_rdog`: 它指向本轮构建为这个集成测试
/// 产出的 bin, 与布局和 hash 都无关。只有它不在时 (二进制被清理、测试可执行
/// 文件被拷出 target 目录) 才回落到按布局探测 —— 探测会看到 `build/rustdog/`
/// 下所有历史 hash 目录, 挑中旧构建的 daemon 等于悄悄换了一个被测对象。
fn rdog_binary_candidates(current_exe: &Path) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut push = |path: PathBuf| {
        if !candidates.contains(&path) {
            candidates.push(path);
        }
    };

    if let Some(cargo_built) = option_env!("CARGO_BIN_EXE_rdog") {
        push(PathBuf::from(cargo_built));
    }

    // 布局无关的基准: 向上回溯到 `target` 目录, 再拼 `{profile}/rdog`。
    if let Some(profile_dir) = target_profile_dir(current_exe) {
        // 旧布局 (`target/{profile}/deps/<test>-<hash>`) 与 cargo 1.100 的
        // bin uplift 都解析到这里。
        push(profile_dir.join(executable_name()));
        push(profile_dir.join("deps").join(executable_name()));
        // build-dir 布局: bin 的 out 目录与测试可执行文件的 hash 目录是兄弟。
        for candidate in build_dir_rdog_candidates(&profile_dir) {
            push(candidate);
        }
    }

    // 直接从测试可执行文件所在目录推导 (布局假设失效时的兜底)。
    if let Some(exe_dir) = current_exe.parent() {
        push(exe_dir.join(executable_name()));
        if let Some(ancestor) = exe_dir.parent() {
            push(ancestor.join(executable_name()));
        }
    }

    candidates
}

/// `.../target/{profile}/...` 里的 `target/{profile}` 目录; 路径中没有 `target`
/// 组成部分 (例如从别处拷出来的测试二进制) 时返回 `None`。
fn target_profile_dir(current_exe: &Path) -> Option<PathBuf> {
    let components: Vec<_> = current_exe.components().collect();
    let target_index = components
        .iter()
        .rposition(|component| component.as_os_str() == "target")?;
    let profile = components.get(target_index + 1)?;
    let target_dir: PathBuf = components[..=target_index].iter().collect();
    Some(target_dir.join(profile.as_os_str()))
}

/// `target/{profile}/build/rustdog/<hash>/out/rdog` 的全部命中 (无命中时为空),
/// 新的在前。只看本包的 build 目录: 同一 `build/` 下还有上百个依赖包的 hash 目录,
/// 它们不可能产出 `rdog`, 全扫只会让「缺二进制」的报错淹没在无关路径里。
fn build_dir_rdog_candidates(profile_dir: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let Ok(hashes) = fs::read_dir(profile_dir.join("build").join(env!("CARGO_PKG_NAME"))) else {
        return candidates;
    };
    for hash in hashes.flatten() {
        candidates.push(hash.path().join("out").join(executable_name()));
    }
    // 新构建优先: 保留的旧 hash 目录里的 daemon 不是本轮的被测对象。
    candidates.sort_by_key(|candidate| {
        std::cmp::Reverse(candidate.metadata().and_then(|meta| meta.modified()).ok())
    });
    candidates
}
