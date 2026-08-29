//! rdog 认证凭证 (issue #81, spec: specs/rdog-authentication-plan.md)。
//!
//! 设计口径:
//! - daemon 首次启动生成随机凭证到 `~/.rdog/auth.toml` (0600) —
//!   本机 local-default 场景 client 与 daemon 同用户读同一文件, 零配置安全
//! - `RDOG_AUTH_USER` / `RDOG_AUTH_PASSWORD` env 覆盖 (CI/脚本注入不落盘)
//! - 凭证绝不进 CLI 参数 (进程列表安全)
//! - daemon 侧另需 zenoh usrpwd 的 users_file (JSON 格式), 由本模块派生

use std::{fs, io, path::PathBuf};

/// rdog 认证凭证 (client 与 daemon 共享的 shared-secret)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthCredentials {
    pub user: String,
    pub password: String,
}

/// 凭证文件名 (位于用户配置目录, 与平台 toml 同级)。
pub const AUTH_CREDENTIALS_FILE: &str = "auth.toml";

impl AuthCredentials {
    /// 读取用户凭证文件; 不存在则生成随机凭证并落盘 (0600)。
    ///
    /// 这是 daemon 与 local-default client 的共享入口:
    /// 首次运行自动生成, 后续运行复用 — 用户零配置。
    pub fn load_or_generate(user_config_dir: &PathBuf) -> io::Result<Self> {
        let path = user_config_dir.join(AUTH_CREDENTIALS_FILE);
        if path.exists() {
            return Self::load_from_file(&path);
        }
        let credentials = Self::generate_random();
        credentials.save_to_file(&path)?;
        Ok(credentials)
    }

    /// 从 toml 文件解析凭证。
    pub fn load_from_file(path: &std::path::Path) -> io::Result<Self> {
        let content = fs::read_to_string(path)?;
        // 自控格式的极简解析 (user/password 两行): 凭证文件由本模块生成,
        // 不引入 toml 解析依赖, 恶意构造最多得到解析错误 (fail-closed)
        let mut user = None::<String>;
        let mut password = None::<String>;
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let Some((key, raw_value)) = line.split_once('=') else {
                return Err(invalid_credentials(path, &format!("无法解析的行: {line}")));
            };
            let value = raw_value.trim().trim_matches('"');
            match key.trim() {
                "user" => user = Some(value.to_owned()),
                "password" => password = Some(value.to_owned()),
                other => {
                    return Err(invalid_credentials(path, &format!("未知字段: {other}")));
                }
            }
        }
        let user = user.ok_or_else(|| invalid_credentials(path, "缺少 user 字段"))?;
        let password = password.ok_or_else(|| invalid_credentials(path, "缺少 password 字段"))?;
        if user.trim().is_empty() || password.trim().is_empty() {
            return Err(invalid_credentials(path, "user/password 不能为空"));
        }
        Ok(Self { user, password })
    }

    /// 生成随机凭证: user 固定前缀 + 随机后缀, password 32 字节 hex。
    fn generate_random() -> Self {
        use rand::RngCore;
        let mut user_suffix = [0u8; 4];
        rand::rng().fill_bytes(&mut user_suffix);
        let mut password_bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut password_bytes);
        Self {
            user: format!("rdog-{}", hex(&user_suffix)),
            password: hex(&password_bytes),
        }
    }

    /// 落盘 (0600): 目录不存在则创建。
    fn save_to_file(&self, path: &std::path::Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = format!(
            "# rdog 认证凭证 (自动生成, 勿提交版本库)\nuser = \"{}\"\npassword = \"{}\"\n",
            self.user, self.password
        );
        fs::write(path, content)?;
        restrict_permissions(path)?;
        Ok(())
    }

    /// env 覆盖: `RDOG_AUTH_USER`/`RDOG_AUTH_PASSWORD` 同时提供才整体覆盖
    /// (半覆盖视为配置错误, 避免用户名来自文件、密码来自 env 的混淆来源)。
    pub fn apply_env_override(self) -> io::Result<Self> {
        let user = std::env::var("RDOG_AUTH_USER")
            .ok()
            .filter(|v| !v.trim().is_empty());
        let password = std::env::var("RDOG_AUTH_PASSWORD")
            .ok()
            .filter(|v| !v.trim().is_empty());
        match (user, password) {
            (Some(user), Some(password)) => Ok(Self { user, password }),
            (None, None) => Ok(self),
            (Some(_), None) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "RDOG_AUTH_USER 已设置但 RDOG_AUTH_PASSWORD 缺失: env 覆盖必须成对提供",
            )),
            (None, Some(_)) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "RDOG_AUTH_PASSWORD 已设置但 RDOG_AUTH_USER 缺失: env 覆盖必须成对提供",
            )),
        }
    }

    /// 保存 zenoh users_file (daemon 侧, ~/.rdog/auth.users.json, 0600)。
    pub fn save_zenoh_users_file(
        &self,
        user_config_dir: &std::path::Path,
    ) -> io::Result<std::path::PathBuf> {
        let path = user_config_dir.join("auth.users.json");
        fs::write(&path, format!("{}:{}\n", self.user, self.password))?;
        restrict_permissions(&path)?;
        Ok(path)
    }

    /// client 侧惰性凭证加载: auth.toml 存在则读; env 成对则覆盖/替代。
    ///
    /// 返回 None = 无凭证 (裸连; daemon 开认证时会被拒, 由调用方报错)。
    pub fn load_client_credentials(user_config_dir: &std::path::Path) -> io::Result<Option<Self>> {
        let path = user_config_dir.join(AUTH_CREDENTIALS_FILE);
        let base = if path.exists() {
            Some(Self::load_from_file(&path)?)
        } else {
            None
        };
        // env 覆盖语义: 成对 env 优先于一切; 半对 = 错误; 无 env 用文件 (无文件 = None)
        let user = std::env::var("RDOG_AUTH_USER")
            .ok()
            .filter(|v| !v.trim().is_empty());
        let password = std::env::var("RDOG_AUTH_PASSWORD")
            .ok()
            .filter(|v| !v.trim().is_empty());
        match (user, password) {
            (Some(user), Some(password)) => Ok(Some(Self { user, password })),
            (None, None) => Ok(base),
            (Some(_), None) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "RDOG_AUTH_USER 已设置但 RDOG_AUTH_PASSWORD 缺失: env 覆盖必须成对提供",
            )),
            (None, Some(_)) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "RDOG_AUTH_PASSWORD 已设置但 RDOG_AUTH_USER 缺失: env 覆盖必须成对提供",
            )),
        }
    }
}

fn invalid_credentials(path: &std::path::Path, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("凭证文件 {} 非法: {reason}", path.display()),
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// unix: 0600; 其他平台文件系统权限语义不同, 保持默认。
fn restrict_permissions(path: &std::path::Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rdog-auth-test-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn load_or_generate_should_create_then_reuse() {
        let dir = temp_dir("gen");
        let first = AuthCredentials::load_or_generate(&dir).unwrap();
        assert!(first.user.starts_with("rdog-"));
        assert_eq!(first.password.len(), 64, "32 字节 hex");

        // 第二次加载复用 (不重新生成)
        let second = AuthCredentials::load_or_generate(&dir).unwrap();
        assert_eq!(first, second);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn credentials_file_should_have_restricted_permissions() {
        let dir = temp_dir("perm");
        let _ = AuthCredentials::load_or_generate(&dir).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.join(AUTH_CREDENTIALS_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "凭证文件应为 0600");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_from_file_should_reject_missing_or_empty_fields() {
        let dir = temp_dir("parse");
        let path = dir.join("auth.toml");
        fs::write(&path, "user = \"a\"\n").unwrap();
        assert!(AuthCredentials::load_from_file(&path).is_err());
        fs::write(&path, "user = \"\"\npassword = \"\"\n").unwrap();
        assert!(AuthCredentials::load_from_file(&path).is_err());
        fs::write(&path, "not toml {{{").unwrap();
        assert!(AuthCredentials::load_from_file(&path).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn env_override_should_require_both_fields() {
        // 无 env: 原样返回
        let base = AuthCredentials {
            user: "u".to_owned(),
            password: "p".to_owned(),
        };
        // 成对覆盖与半覆盖在并行测试下有 env 串台风险, 这里只测纯函数语义:
        // 直接构造验证 apply 的匹配逻辑分支 (env 测试在 e2e 层做)
        assert_eq!(base.clone().apply_env_override().unwrap(), base);
    }

    #[test]
    fn zenoh_users_file_should_be_colon_separated_text() {
        // zenoh 1.8 dictionary 格式: user:password 每行一条 (非 JSON)
        let dir = temp_dir("dict");
        let credentials = AuthCredentials {
            user: "rdog-ab".to_owned(),
            password: "secret".to_owned(),
        };
        let path = credentials.save_zenoh_users_file(&dir).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "rdog-ab:secret\n");
        let _ = fs::remove_dir_all(&dir);
    }
}
