//! TLS 证书材料生成 (`rdog auth tls-init`, issue #87 / spec: specs/rdog-tls-plan.md)。
//!
//! 设计口径:
//! - rcgen 在 Rust 内生成自建 CA + daemon 证书 + mTLS 客户端套件 —
//!   零配置哲学延续 (不依赖 minica/openssl 外部工具)
//! - 布局: ~/.rdog/tls/{ca.pem, ca-key.pem, daemon/{cert,key}.pem,
//!   client/{cert,key}.pem}, 私钥 0600
//! - 幂等: 已存在则跳过不覆盖 (用户材料是手工调过的不能静默毁掉)。**`tls/` 目录永远不会被
//!   替换**: 六件齐备就跳过, 不齐就报错点名缺哪几件 —— 想重新生成必须先由人删掉该目录, 代价是
//!   显式的一步而不是某个 rename 顺手做掉
//! - 发布方式是原子的: 六件先写进同目录的 `<tls>.staging-<pid>-<n>`, 再整目录 rename 到位,
//!   所以"目录存在"对新一代写入方等价于"材料完整" (见 `tls_init` 的注释)
//! - daemon 证书 SAN 覆盖 hostname + 127.0.0.1 (zenoh 0.7.1+ 支持 IP SAN)

use std::{fs, io, path::Path};

use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DistinguishedName, DnType, IsCa, KeyPair,
};

/// tls 材料目录名 (位于用户配置目录下)。
pub const TLS_DIR_NAME: &str = "tls";

/// `rdog auth tls-init` 的生成结果 (哪些已存在被跳过)。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TlsInitReport {
    pub created: Vec<String>,
    pub skipped: Vec<String>,
}

/// 材料布局 (spec 固定): 六件齐备才算初始化完成。
pub const TLS_MATERIAL_FILES: [&str; 6] = [
    "ca.pem",
    "ca-key.pem",
    "daemon/cert.pem",
    "daemon/key.pem",
    "client/cert.pem",
    "client/key.pem",
];

/// 生成全部 TLS 材料 (幂等)。
pub fn tls_init(tls_dir: &Path) -> io::Result<TlsInitReport> {
    let mut report = TlsInitReport::default();
    if tls_dir.exists() {
        // 幂等: 已初始化的目录一个文件都不覆盖 (用户材料是手工调过的不能静默毁掉)。
        // 但"目录在"不等于"材料完整": 旧版本是边建目录边写文件, 中途崩溃/被杀会留下
        // 半套材料, 而按半套材料做 TLS 握手必然失败、还会把排查引向"证书不对"。
        // 因此这里 fail closed, 由操作者决定是删掉重生成还是补文件。
        let missing: Vec<&str> = TLS_MATERIAL_FILES
            .iter()
            .copied()
            .filter(|relative| !tls_dir.join(relative).is_file())
            .collect();
        if !missing.is_empty() {
            return Err(io::Error::other(format!(
                "TLS 材料不完整 ({}): 缺少 {}; 确认无进程在用后删除该目录再重跑 `rdog auth tls-init`",
                tls_dir.display(),
                missing.join(", ")
            )));
        }
        report.skipped.push(tls_dir.display().to_string());
        return Ok(report);
    }

    // 自建 CA (minica 同款语义: CA 操作者运营各主机)
    let ca_key = KeyPair::generate().map_err(to_io_error)?;
    let mut ca_params = CertificateParams::new(Vec::new()).map_err(to_io_error)?;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.distinguished_name = distinguished_name("rdog internal CA");
    let ca = CertifiedIssuer::self_signed(ca_params, &ca_key).map_err(to_io_error)?;

    // daemon (服务器) 证书: SAN = hostname + 127.0.0.1 + localhost
    let daemon_params = CertificateParams::new(vec![
        hostname_or_localhost(),
        "127.0.0.1".to_owned(),
        "localhost".to_owned(),
    ])
    .map_err(to_io_error)?;
    let daemon_key = KeyPair::generate().map_err(to_io_error)?;
    let daemon_cert =
        CertifiedIssuer::signed_by(daemon_params, &daemon_key, &ca).map_err(to_io_error)?;

    // mTLS 客户端证书
    let client_params = CertificateParams::new(Vec::new()).map_err(to_io_error)?;
    let client_key = KeyPair::generate().map_err(to_io_error)?;
    let client_cert =
        CertifiedIssuer::signed_by(client_params, &client_key, &ca).map_err(to_io_error)?;

    // 落盘布局 (spec 固定)。先在唯一暂存目录里写完六件, 再整目录 rename 到位:
    // `<tls_dir>` 目录出现即等价于材料完整, 两个并发 tls-init 只可能有一方 rename 成功,
    // 落败方按已初始化跳过。边创建边写入会让两个进程的 CA 与证书交错落进同一目录,
    // daemon 证书签自 CA-A 而 ca.pem 来自 CA-B —— TLS/mTLS 握手必然失败。
    let staging_dir = create_staging_dir(tls_dir)?;
    let materialize = || -> io::Result<()> {
        let daemon_dir = staging_dir.join("daemon");
        let client_dir = staging_dir.join("client");
        fs::create_dir_all(&daemon_dir)?;
        fs::create_dir_all(&client_dir)?;

        write_private(&staging_dir.join("ca-key.pem"), &ca_key.serialize_pem())?;
        write_public(&staging_dir.join("ca.pem"), &ca.as_ref().pem())?;
        write_private(&daemon_dir.join("key.pem"), &daemon_key.serialize_pem())?;
        write_public(&daemon_dir.join("cert.pem"), &daemon_cert.pem())?;
        write_private(&client_dir.join("key.pem"), &client_key.serialize_pem())?;
        write_public(&client_dir.join("cert.pem"), &client_cert.pem())?;
        Ok(())
    };
    if let Err(err) = materialize().and_then(|()| fs::rename(&staging_dir, tls_dir)) {
        let _ = fs::remove_dir_all(&staging_dir);
        if tls_dir.exists() {
            // 并发方先落地: 它的材料是完整的, 本进程按已初始化跳过。
            report.skipped.push(tls_dir.display().to_string());
            return Ok(report);
        }
        return Err(err);
    }

    report.created = TLS_MATERIAL_FILES
        .iter()
        .map(|path| path.to_string())
        .collect();
    Ok(report)
}

/// 建一个本次调用独占的暂存目录。
///
/// 名字里的 pid + 时间戳只提供低概率唯一, 所以用 `create_dir` (目标已存在即失败) 加上
/// 进程内计数器重试: 唯一性必须是机制, 而不是"两次调用不会撞在同一纳秒"的巧合 —
/// 撞名时两个调用会共享暂存目录, 一方的 `remove_dir_all` 会删掉另一方正在写的材料。
fn create_staging_dir(tls_dir: &Path) -> io::Result<std::path::PathBuf> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static ATTEMPT: AtomicU32 = AtomicU32::new(0);

    let name = tls_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| TLS_DIR_NAME.to_owned());
    loop {
        let attempt = ATTEMPT.fetch_add(1, Ordering::Relaxed);
        let candidate =
            tls_dir.with_file_name(format!("{name}.staging-{}-{attempt}", std::process::id()));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                // 父目录 (通常是 ~/.rdog) 还不存在: 用它建父链后再重试。
                if let Some(parent) = candidate.parent() {
                    fs::create_dir_all(parent)?;
                }
                continue;
            }
            Err(err) => return Err(err),
        }
    }
}

fn distinguished_name(common_name: &str) -> DistinguishedName {
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, common_name);
    name
}

fn hostname_or_localhost() -> String {
    // 不引入 hostname crate: env 优先, 无则 SAN 里已有 localhost/127.0.0.1 兜底。
    // (分布式部署时用户可用 tls-init 后手工补签多 SAN 证书)
    std::env::var("HOSTNAME")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "localhost".to_owned())
}

fn write_public(path: &Path, content: &str) -> io::Result<()> {
    fs::write(path, content)
}

fn write_private(path: &Path, content: &str) -> io::Result<()> {
    fs::write(path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn to_io_error(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tls_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rdog-tls-test-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn tls_init_should_generate_full_layout() {
        let dir = temp_tls_dir("layout");
        let report = tls_init(&dir).unwrap();
        assert_eq!(report.created.len(), 6, "{report:?}");
        assert!(report.skipped.is_empty());

        for path in [
            "ca.pem",
            "ca-key.pem",
            "daemon/cert.pem",
            "daemon/key.pem",
            "client/cert.pem",
            "client/key.pem",
        ] {
            assert!(dir.join(path).exists(), "missing {path}");
        }

        // PEM 头语义可辨 (openssl 兼容形状)
        let ca = fs::read_to_string(dir.join("ca.pem")).unwrap();
        assert!(ca.starts_with("-----BEGIN CERTIFICATE-----"), "{ca:.40}");
        let key = fs::read_to_string(dir.join("ca-key.pem")).unwrap();
        assert!(key.contains("PRIVATE KEY"), "{key:.40}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tls_init_should_be_idempotent_and_never_overwrite() {
        let dir = temp_tls_dir("idem");
        tls_init(&dir).unwrap();
        // 手工标记用户材料
        fs::write(dir.join("ca.pem"), "USER TWEAKED").unwrap();
        let report = tls_init(&dir).unwrap();
        assert_eq!(report.skipped.len(), 1, "{report:?}");
        assert!(report.created.is_empty());
        assert_eq!(
            fs::read_to_string(dir.join("ca.pem")).unwrap(),
            "USER TWEAKED",
            "已存在材料不得被覆盖"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 两个 `tls-init` 并发时只能有一方落盘。
    ///
    /// 反例 (2026-11-26 实测): 目录先创建、文件后写入, 且"已初始化"的判据是
    /// `tls_dir.exists()`。两个进程同时进入时, 要么双方都生成、把两份 CA 的证书
    /// 交错写进同一目录 (daemon 证书签自 CA-A, ca.pem 来自 CA-B), 要么后者看到
    /// 空目录就跳过、留下半套材料。两种结局都让 TLS/mTLS 握手必然失败。
    ///
    /// 暂存目录的检查只扫本用例自持的父目录: `temp_dir()` 是进程级共享命名空间,
    /// 同二进制里别的用例的 `<...>.staging-*` 也落在那里, 按名字在 `/tmp` 上过滤会把
    /// 兄弟用例的正常中间态当成泄漏 (见 parallel-test-global-state-single-lock)。
    #[test]
    fn concurrent_tls_init_should_publish_exactly_one_material_set() {
        let root = temp_tls_dir("concurrent");
        let dir = root.join(TLS_DIR_NAME);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let dir = dir.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    tls_init(&dir).expect("concurrent tls_init should not fail")
                })
            })
            .collect();
        let reports: Vec<TlsInitReport> = handles
            .into_iter()
            .map(|handle| handle.join().expect("tls_init thread should not panic"))
            .collect();

        let created = reports.iter().filter(|r| !r.created.is_empty()).count();
        assert_eq!(
            created, 1,
            "并发 tls-init 只能有一方生成完整材料, 实际 {created}: {reports:?}"
        );
        for path in TLS_MATERIAL_FILES {
            assert!(dir.join(path).is_file(), "missing {path}");
        }

        // 落败的并发方必须收走自己的暂存目录, 不在 ~/.rdog 里留残渣。
        let leftovers: Vec<_> = fs::read_dir(&root)
            .expect("root dir should be readable")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name != TLS_DIR_NAME)
            .collect();
        assert!(leftovers.is_empty(), "暂存目录残留: {leftovers:?}");

        let _ = fs::remove_dir_all(&root);
    }

    /// 已存在的材料目录永远不会被替换 —— 连 rename 都不会发生。
    ///
    /// 判据用 inode: 整目录 rename 到位会换掉目录 inode, 而"跳过"不会动它。这条同时挡住
    /// 两类误读: ① 把 `tls-init` 当成"重新生成"的人以为它会顺手刷新; ② 有人在目录里放了
    /// 自己的标记文件, 被一次 rename 静默清掉。想重新生成必须先人工删目录 (显式的一步)。
    #[test]
    fn tls_init_should_never_replace_existing_material_directory() {
        let dir = temp_tls_dir("no-replace");
        tls_init(&dir).unwrap();
        fs::write(dir.join("ca.pem"), "USER TWEAKED").unwrap();
        fs::write(dir.join("keep.txt"), "user marker").unwrap();
        #[cfg(unix)]
        let inode_before = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(&dir).unwrap().ino()
        };

        let report = tls_init(&dir).unwrap();
        assert!(
            report.created.is_empty(),
            "已存在材料不应被重新生成: {report:?}"
        );
        assert_eq!(report.skipped.len(), 1, "{report:?}");
        assert_eq!(
            fs::read_to_string(dir.join("ca.pem")).unwrap(),
            "USER TWEAKED",
            "已存在材料不得被覆盖"
        );
        assert!(
            dir.join("keep.txt").is_file(),
            "目录被整目录替换了: 用户的标记文件随之消失"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                fs::metadata(&dir).unwrap().ino(),
                inode_before,
                "目录 inode 变了 = 发生过 rename 替换"
            );
        }

        let _ = fs::remove_dir_all(&dir);
    }

    /// 半套材料必须 fail closed, 而不是被当成"已初始化"跳过。
    ///
    /// 与并发用例不同, 这条与调度无关: 旧版本边建目录边写文件, 中途崩溃就会留下
    /// 只有部分文件的目录。此时既不能静默跳过 (后续 TLS 握手必然失败, 排查会被引向
    /// "证书不对"), 也不能擅自覆盖 (用户材料可能手工调过), 所以直接报错点名缺哪几件。
    #[test]
    fn tls_init_should_reject_incomplete_existing_material() {
        let dir = temp_tls_dir("incomplete");
        fs::create_dir_all(dir.join("daemon")).unwrap();
        fs::write(dir.join("ca.pem"), "-----BEGIN CERTIFICATE-----").unwrap();

        let err = tls_init(&dir).expect_err("半套材料不应被当成已初始化");
        let message = err.to_string();
        for missing in ["ca-key.pem", "daemon/cert.pem", "client/cert.pem"] {
            assert!(
                message.contains(missing),
                "错误应点名缺失项 {missing}: {message}"
            );
        }
        // 报错路径不得改动既有文件。
        assert_eq!(
            fs::read_to_string(dir.join("ca.pem")).unwrap(),
            "-----BEGIN CERTIFICATE-----"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn private_keys_should_have_restricted_permissions() {
        let dir = temp_tls_dir("perm");
        tls_init(&dir).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in ["ca-key.pem", "daemon/key.pem", "client/key.pem"] {
                let mode = fs::metadata(dir.join(path)).unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o600, "{path} 应为 0600");
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
