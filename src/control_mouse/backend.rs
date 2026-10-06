use super::{
    plan::{MouseExecutionPlan, MousePlanStep},
    report::{MouseExecutionReport, MouseReleaseRecovery},
};
use crate::control_display_scope::display_contains_point;
use crate::control_mouse::request::MousePoint;
use crate::control_overlay;
use enigo::{Axis, Button, Coordinate, Direction, Enigo, InputError, Mouse};
use std::{io, thread, time::Duration};

/// 绝对移动后端**真正交给操作系统**的那个点 (回执 `resolved_os_point` 的数据源)。
///
/// # 为什么回执必须是它而不是 plan 里的协议点 (2026-10-06 真机取证)
///
/// `MousePlanStep::Move` 里装的是协议 os-logical 坐标, 但那个数字**不等于**落点:
/// macOS/Linux 走 enigo 原样透传 (落点 = 协议点); Windows 走 WinNative 后端,
/// 内部按命中屏的 scale 换算成物理像素才交给 `SetCursorPos`。
/// 回执此前记的是换算前的协议点 —— 在 4K@200% 的机器上等于报了一个"从没点过
/// 的位置", agent 拿它当"实际点了屏幕哪里"看必然被带偏 (实际点的是双倍坐标)。
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct AppliedPoint {
    pub point: MousePoint,
    /// 落点自身所处的空间, 由后端如实申报, 不由调用方推断。
    pub space: AppliedPointSpace,
}

/// 落点所处的坐标空间。与回执顶层的 `coordinate_space` (请求空间) **不是**
/// 一回事 —— 请求永远是 os-logical, 落点则取决于后端怎么解释它。
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AppliedPointSpace {
    /// OS 逻辑坐标 (`os-logical`): macOS 的 points、X11 的像素, 与协议坐标同空间。
    OsLogical,
    /// 物理像素: Windows 原生后端 `SetCursorPos` 接收的空间。
    Physical,
}

impl AppliedPointSpace {
    pub fn as_protocol_str(self) -> &'static str {
        match self {
            Self::OsLogical => "os-logical",
            Self::Physical => "physical",
        }
    }
}

pub trait MouseBackend {
    /// 移动鼠标。
    ///
    /// `Coordinate::Abs` 必须回传**实际发给 OS** 的点与它的坐标空间 (回执靠它
    /// 如实报告落点); `Coordinate::Rel` 回 `None` —— 后端无从得知绝对落点,
    /// 不猜、不造。
    fn move_mouse(
        &mut self,
        x: i32,
        y: i32,
        coordinate: Coordinate,
    ) -> Result<Option<AppliedPoint>, InputError>;
    fn button(&mut self, button: Button, direction: Direction) -> Result<(), InputError>;
    fn scroll(&mut self, length: i32, axis: Axis) -> Result<(), InputError>;
}

/// 当前平台**真正发事件**的鼠标后端名 —— 回执 `backend` 字段的单一真相源。
///
/// # 为什么需要它 (2026-10-06 真机取证)
///
/// Windows 上的 `@click` 走的是 `WinNativeMouseBackend` (`SetCursorPos` +
/// `SendInput`), 不是 enigo。但回执里 `backend` 字段一直写死报 `"enigo"`
/// —— 真机 4K@200% 上实测点得中 (行为对), 标签却在说谎。agent 拿这个字段
/// 判断"到底谁发的物理坐标"时会被误导, 排 DPI 类问题会被直接带偏。
///
/// # 与分发点的耦合契约
///
/// 本常量必须与**唯一实例化后端的地方**成对修改 ——
/// `control_actions::execute_mouse_plan` 的 `#[cfg(windows)]` 分支实例化
/// `WinNativeMouseBackend`, `#[cfg(not(windows))]` 分支实例化 `Enigo`。
/// 改那一侧的分发必须同时改这里的 cfg, 否则标签与实际路径再次脱节。
///
/// 注意这条"成对修改"靠人守: 常量与分发点是两处 cfg, 编译器不会拦。
/// 回归断言 (`plan::tests::mouse_receipt_reports_the_backend_that_actually_runs`)
/// 只在**标签与平台真值不一致的平台上**才咬人 —— 在 macOS 上期望值本身就是
/// "enigo", 把写死的 "enigo" 换回去测试照样绿 (2026-10-06 实测确认)。
/// 即 Windows 侧是否真按预期编译并跑过该用例, 需在 Windows 目标上验证。
#[cfg(windows)]
pub const MOUSE_BACKEND_NAME: &str = "win-native";

#[cfg(not(windows))]
pub const MOUSE_BACKEND_NAME: &str = "enigo";

impl MouseBackend for Enigo {
    fn move_mouse(
        &mut self,
        x: i32,
        y: i32,
        coordinate: Coordinate,
    ) -> Result<Option<AppliedPoint>, InputError> {
        Mouse::move_mouse(self, x, y, coordinate)?;
        // enigo 原样透传协议坐标, OS 侧就在这个空间里解读它
        // (macOS points / X11 像素), 所以落点 = 入参, 无换算可言。
        Ok((coordinate == Coordinate::Abs).then_some(AppliedPoint {
            point: MousePoint { x, y },
            space: AppliedPointSpace::OsLogical,
        }))
    }

    fn button(&mut self, button: Button, direction: Direction) -> Result<(), InputError> {
        Mouse::button(self, button, direction)
    }

    fn scroll(&mut self, length: i32, axis: Axis) -> Result<(), InputError> {
        Mouse::scroll(self, length, axis)
    }
}

/// 执行鼠标 plan,并在组合动作中尽量修复遗留按下状态。
///
/// `@mouse-button mode:"press"` 的 plan 没有后续步骤,因此不会被这里自动 release。
/// 只有 press 后的后续 step 失败时,才执行恢复 release。
pub fn perform_mouse_plan<B: MouseBackend>(
    backend: &mut B,
    plan: &MouseExecutionPlan,
) -> io::Result<MouseExecutionReport> {
    let mut pressed_button = None::<Button>;
    // 最近一次绝对移动的两个视角, 消费者不同不能混用:
    // - 协议点 (os-logical): 给 overlay 闪烁提示, 它按 os-logical 契约收坐标;
    // - 后端回报的落点: 给回执, Windows 上是换算后的物理像素。
    let mut last_protocol_point = None::<MousePoint>;
    let mut last_applied_point = None::<AppliedPoint>;

    for step in &plan.steps {
        let result = match *step {
            MousePlanStep::Move { x, y, coordinate } => match backend.move_mouse(x, y, coordinate) {
                Ok(applied) => {
                    if coordinate == Coordinate::Abs {
                        last_protocol_point = Some(MousePoint { x, y });
                        last_applied_point = applied;
                    }
                    // 实测 (#102): warp/moved 事件与紧随的 click 之间需要让
                    // WindowServer 收敛, 否则 click 落在移动前的旧位置
                    // (跨显示器移动尤甚, 表现为两次点击都落在 one 位置)。
                    thread::sleep(Duration::from_millis(40));
                    Ok(())
                }
                Err(err) => Err(err),
            },
            MousePlanStep::Button { button, direction } => {
                let result = backend.button(button, direction);
                if result.is_ok() {
                    // 可视化反馈: 点击/按下时在屏幕上闪烁标记, 让用户看到 agent 动作
                    if matches!(direction, Direction::Click | Direction::Press) {
                        if let Some(point) = last_protocol_point {
                            control_overlay::flash_click(point.x, point.y);
                        }
                    }
                    match direction {
                        Direction::Press => pressed_button = Some(button),
                        Direction::Release if pressed_button == Some(button) => {
                            pressed_button = None
                        }
                        Direction::Click | Direction::Release => {}
                    }
                }
                result
            }
            MousePlanStep::Hold(hold_ms) => {
                thread::sleep(Duration::from_millis(hold_ms));
                Ok(())
            }
            MousePlanStep::Scroll { length, axis } => backend.scroll(length, axis),
        };

        if let Err(err) = result {
            return recover_after_mouse_failure(backend, pressed_button, err);
        }
    }

    let mut report = plan.report.clone();
    enrich_report_with_display(&mut report, last_applied_point);
    Ok(report)
}

/// 用后端回报的真实落点补全回执, 并挂上 display catalog 的归属信息。
///
/// 落点与 catalog 必须在**同一空间**里比对 —— 这由平台自身保证:
/// macOS 的 `DisplaySummary.os_rect` 是 points (协议逻辑坐标), Windows
/// DPI-aware 进程里是物理像素 (物理落点), X11 像素即逻辑
/// (见 `screenshot::LogicalRect::to_os_logical` 头注的平台取证)。
///
/// 失败静默: 归属信息是**增强证据**, 拿不到 (catalog 枚举失败/无命中屏)
/// 不该让一次成功的点击变成失败 —— 坐标本身已经执行成功。
/// 无命中屏 (坐标在所有屏之外) 同样静默, agent 可从缺字段判断异常。
fn enrich_report_with_display(report: &mut MouseExecutionReport, applied: Option<AppliedPoint>) {
    let Some(applied) = applied else {
        return;
    };
    let MousePoint { x, y } = applied.point;
    report.resolved_os_point = Some(applied.point);
    report.resolved_os_point_space = Some(applied.space);
    let Ok(displays) = crate::screenshot::current_display_summaries() else {
        return;
    };
    let Some(display) = displays
        .iter()
        .find(|display| display_contains_point(display, x, y))
    else {
        return;
    };
    report.display_id = Some(display.display_id.clone());
    report.applied_scale_factor = Some(display.scale_factor);
}

fn recover_after_mouse_failure<B: MouseBackend>(
    backend: &mut B,
    pressed_button: Option<Button>,
    err: InputError,
) -> io::Result<MouseExecutionReport> {
    let Some(button) = pressed_button else {
        return Err(io::Error::other(err.to_string()));
    };

    let (recovery, release_error) = match backend.button(button, Direction::Release) {
        Ok(()) => (MouseReleaseRecovery::ReleaseSucceeded, None),
        Err(release_err) => (MouseReleaseRecovery::ReleaseFailed, Some(release_err)),
    };
    let message = match (recovery, release_error) {
        (MouseReleaseRecovery::ReleaseSucceeded, None) => {
            format!("{err}; release_recovery=release-succeeded; 已在失败恢复中释放鼠标按钮")
        }
        (MouseReleaseRecovery::ReleaseFailed, Some(release_err)) => {
            format!("{err}; release_recovery=release-failed; 已尝试释放鼠标按钮,但 release 也失败: {release_err}")
        }
        _ => format!("{err}; release_recovery=unknown"),
    };

    Err(io::Error::other(message))
}
