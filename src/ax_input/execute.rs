//! type-text 投递策略: 模式分发与 Auto 回退链。
//!
//! 策略自 control_ax/macos.rs 迁入 (平台无关的投递编排); 三条平台路径
//! (AXValue / targeted keyboard / clipboard) 与信任检查由调用方注入,
//! 使回退边界与错误命名可在无 macOS 环境下单测。

use crate::control_ax::types::{TypeTextMode, TypeTextReport, TypeTextRequest};
use std::io;

/// 按模式分发 type-text 投递, Auto 模式按"可恢复投递错误"逐层回退。
///
/// 回退顺序: AXValue -> targeted keyboard -> clipboard (需 allow_clipboard)。
/// 权限、IO 等不可恢复错误原样返回, 不绕过用户或系统的安全边界。
pub(crate) fn type_text_with_delivery_paths(
    request: &TypeTextRequest,
    ensure_ready: impl FnOnce() -> io::Result<()>,
    via_ax_value: impl FnOnce(&TypeTextRequest) -> io::Result<TypeTextReport>,
    via_targeted_keyboard: impl FnOnce(&TypeTextRequest) -> io::Result<TypeTextReport>,
    via_clipboard: impl FnOnce(&TypeTextRequest) -> io::Result<TypeTextReport>,
) -> io::Result<TypeTextReport> {
    ensure_ready()?;

    match request.mode {
        // 显式 AXValue 模式保持协议命名错误; 显式 keyboard/clipboard
        // 模式历史上不做 remap, 维持原样。
        TypeTextMode::AxValue => {
            via_ax_value(request).map_err(|error| remap_type_text_path_error(error, "AXValue"))
        }
        TypeTextMode::TargetedKeyboard => via_targeted_keyboard(request),
        TypeTextMode::Clipboard => via_clipboard(request),
        TypeTextMode::Auto => match via_ax_value(request) {
            Ok(report) => Ok(report),
            Err(ax_err) if can_fallback_type_text_delivery(&ax_err) => {
                match via_targeted_keyboard(request) {
                    Ok(report) => Ok(report),
                    Err(keyboard_err)
                        if request.allow_clipboard
                            && can_fallback_type_text_delivery(&keyboard_err) =>
                    {
                        via_clipboard(request)
                    }
                    Err(keyboard_err) => Err(remap_type_text_path_error(
                        keyboard_err,
                        "targeted keyboard",
                    )),
                }
            }
            Err(ax_err) => Err(remap_type_text_path_error(ax_err, "AXValue")),
        },
    }
}

/// 仅把"不支持此投递方式"作为 auto 的下一层尝试条件。
/// 权限、IO 等失败必须原样返回,不能绕过用户或系统的安全边界。
pub(crate) fn can_fallback_type_text_delivery(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported
    )
}

/// 把 type-text 底层路径 (AXValue / targeted keyboard) 的错误包装成协议命名的错误,
/// 保留原有 ErrorKind, 只改写用户可见的 message。
/// 两条路径的包装规则逐字相同, 只有路径名不同, 共享这一份实现。
pub(crate) fn remap_type_text_path_error(err: io::Error, path_label: &str) -> io::Error {
    let message = err.to_string();
    match err.kind() {
        io::ErrorKind::Unsupported => io::Error::new(
            io::ErrorKind::Unsupported,
            format!("type-text 当前只支持 macOS {path_label} 路径"),
        ),
        io::ErrorKind::InvalidInput => io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("type-text {path_label} 路径失败: {message}"),
        ),
        io::ErrorKind::PermissionDenied => io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("type-text {path_label} 路径失败: {message}"),
        ),
        _ => io::Error::other(format!("type-text {path_label} 路径失败: {message}")),
    }
}

#[cfg(test)]
mod delivery_policy_tests {
    use super::*;

    fn request(mode: TypeTextMode, allow_clipboard: bool) -> TypeTextRequest {
        TypeTextRequest {
            target: Default::default(),
            text: "hello".to_owned(),
            mode,
            allow_clipboard,
        }
    }

    /// 回退边界: 只有 InvalidInput / Unsupported 算可恢复投递错误,
    /// 权限与 IO 失败必须原样上抛 (自 macos.rs 测试迁入)。
    #[test]
    fn auto_should_only_fallback_after_recoverable_delivery_errors() {
        assert!(can_fallback_type_text_delivery(&io::Error::new(
            io::ErrorKind::InvalidInput,
            "AXValue 不可写"
        )));
        assert!(can_fallback_type_text_delivery(&io::Error::new(
            io::ErrorKind::Unsupported,
            "AXValue 不支持"
        )));
        assert!(!can_fallback_type_text_delivery(&io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Accessibility 未授权"
        )));
        assert!(!can_fallback_type_text_delivery(&io::Error::other(
            "CGEvent 传输失败"
        )));
    }

    /// Auto 链路: AXValue 可恢复失败 -> targeted keyboard 接管, 不再触达 clipboard。
    #[test]
    fn auto_should_fall_through_to_keyboard_when_ax_value_is_recoverable() {
        let report = type_text_with_delivery_paths(
            &request(TypeTextMode::Auto, true),
            || Ok(()),
            |_| Err(io::Error::new(io::ErrorKind::Unsupported, "AXValue 不可写")),
            |req| {
                assert_eq!(req.text, "hello");
                Ok(TypeTextReport::targeted_keyboard_success("test", None))
            },
            |_| panic!("keyboard 成功后不应触达 clipboard"),
        )
        .unwrap();
        assert_eq!(report.backend, "test");
    }

    /// 权限失败不回退: 第一条路径的权限错误必须原样上抛 (安全边界)。
    #[test]
    fn auto_should_not_fallback_on_permission_denied() {
        let error = type_text_with_delivery_paths(
            &request(TypeTextMode::Auto, true),
            || Ok(()),
            |_| Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
            |_| panic!("权限失败不得回退到 keyboard"),
            |_| panic!("权限失败不得回退到 clipboard"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    /// clipboard 门禁: allow_clipboard=false 时 keyboard 失败直接带命名错误上抛。
    #[test]
    fn auto_should_respect_clipboard_gate() {
        let error = type_text_with_delivery_paths(
            &request(TypeTextMode::Auto, false),
            || Ok(()),
            |_| Err(io::Error::new(io::ErrorKind::Unsupported, "AXValue 不可写")),
            |_| Err(io::Error::new(io::ErrorKind::Unsupported, "键盘不可用")),
            |_| panic!("allow_clipboard=false 时不得触达 clipboard"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(
            error
                .to_string()
                .contains("type-text 当前只支持 macOS targeted keyboard 路径"),
            "unexpected error: {error}"
        );
    }

    /// remap 必须保留 ErrorKind 并改写成 type-text 协议命名 (自 macos.rs 测试迁入)。
    /// 收敛为单函数后, 两条路径标签的输出都要逐字保持原契约。
    #[test]
    fn remap_type_text_path_error_should_use_type_text_protocol_name() {
        let unsupported = remap_type_text_path_error(
            io::Error::new(io::ErrorKind::Unsupported, "AX set value 当前只支持 macOS"),
            "AXValue",
        );
        assert_eq!(unsupported.kind(), io::ErrorKind::Unsupported);
        assert!(
            unsupported
                .to_string()
                .contains("type-text 当前只支持 macOS AXValue 路径"),
            "unexpected error: {unsupported}"
        );

        let invalid = remap_type_text_path_error(
            io::Error::new(io::ErrorKind::InvalidInput, "目标 AX 元素不支持 AXValue"),
            "AXValue",
        );
        assert_eq!(invalid.kind(), io::ErrorKind::InvalidInput);
        assert!(
            invalid.to_string().contains("type-text AXValue 路径失败"),
            "unexpected error: {invalid}"
        );

        // targeted keyboard 路径标签同样进入同一份包装规则。
        let keyboard = remap_type_text_path_error(
            io::Error::new(io::ErrorKind::PermissionDenied, "AX isolated"),
            "targeted keyboard",
        );
        assert_eq!(keyboard.kind(), io::ErrorKind::PermissionDenied);
        assert!(
            keyboard
                .to_string()
                .contains("type-text targeted keyboard 路径失败"),
            "unexpected error: {keyboard}"
        );
    }
}
