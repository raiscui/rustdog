//! TLS 证书材料生成 (`rdog auth tls-init`, issue #87 / spec: specs/rdog-tls-plan.md)。
//!
//! 设计口径:
//! - rcgen 在 Rust 内生成自建 CA + daemon 证书 + mTLS 客户端套件 —
//!   零配置哲学延续 (不依赖 minica/openssl 外部工具)
//! - 布局: ~/.rdog/tls/{ca.pem, ca-key.pem, daemon/{cert,key}.pem,
//!   client/{cert,key}.pem}, 私钥 0600
//! - 幂等: 已存在则跳过不覆盖 (用户材料是手工调过的不能静默毁掉)
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

/// 生成全部 TLS 材料 (幂等)。
pub fn tls_init(tls_dir: &Path) -> io::Result<TlsInitReport> {
    let mut report = TlsInitReport::default();
    if tls_dir.exists() {
        // 幂等: 目录存在即视为已初始化, 不覆盖任何文件
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

    // 落盘布局 (spec 固定)
    let daemon_dir = tls_dir.join("daemon");
    let client_dir = tls_dir.join("client");
    fs::create_dir_all(&daemon_dir)?;
    fs::create_dir_all(&client_dir)?;

    write_private(&tls_dir.join("ca-key.pem"), &ca_key.serialize_pem())?;
    write_public(&tls_dir.join("ca.pem"), &ca.as_ref().pem())?;
    write_private(&daemon_dir.join("key.pem"), &daemon_key.serialize_pem())?;
    write_public(&daemon_dir.join("cert.pem"), &daemon_cert.pem())?;
    write_private(&client_dir.join("key.pem"), &client_key.serialize_pem())?;
    write_public(&client_dir.join("cert.pem"), &client_cert.pem())?;

    report.created = vec![
        "ca.pem".into(),
        "ca-key.pem".into(),
        "daemon/cert.pem".into(),
        "daemon/key.pem".into(),
        "client/cert.pem".into(),
        "client/key.pem".into(),
    ];
    Ok(report)
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
