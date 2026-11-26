//! 本进程的 build 身份。
//!
//! 读数必须能回答"这是哪个构建量的": `rdog.bulk-path.v1` 的两半各带自己的 `build`, 两个 sha 不
//! 相同就说明两端不是同一次构建 —— 那条读数作废 (RAI-476 的 A 类验收纪律)。
//!
//! 来源: `build.rs` 在编译期写入 `GIT_SHA` (CI 可用同名环境变量覆盖), 没有 git 时是 `unknown`。
//! 宁可写 `unknown` 也不猜: 一个编出来的 sha 会让"两端同源"这条判断变成假的。

/// 编译期确定的 git 提交号 (7 位), 或 `unknown`。
pub(crate) const BUILD_SHA: &str = env!("GIT_SHA");

/// 一行读数里用的短形式: `build=abc1234`。
pub(crate) fn build_tag() -> String {
    format!("build={BUILD_SHA}")
}
