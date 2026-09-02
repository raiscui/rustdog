// ============================================================================
// 可视化反馈层 (overlay): 让人类用户实时看到 agent 的局部截图与点击动作。
//
// - @screenshot include_ocr: 屏幕上半透明绿框标出识别到的文本位置
// - @click / @mouse-button: 点击点橙色方块闪烁
// - 窗口截图: 淡蓝色标出截取的区域范围
//
// 架构 (重要): AppKit 严格要求 UI 在进程主线程, 而 daemon 主线程是 tokio
// runtime; 实测在非主线程创建 NSPanel 会触发 Objective-C 异常, Rust 无法
// 捕获 C++ 异常直接 abort 整个 daemon。因此 overlay 拆为子进程:
//
//   daemon --stdin JSON 行--> `rdog overlay` 子进程 (自己的主线程跑 AppKit)
//
// 子进程在 stdin EOF (daemon 退出) 后自动结束; 进程边界隔离任何 AppKit 崩溃,
// 可视化永不影响控制主流程。开关: RDOG_OVERLAY=0/false 关闭。非 macOS no-op。
// ============================================================================

use serde::Deserialize;
use std::io;

/// 识别框数量上限 (超长按钮列表截断)
const BOX_CAP: usize = 32;
/// 面板初始不透明度 (后 40% 存活期线性淡出)
const BASE_ALPHA: f64 = 0.45;

/// 事件 JSON (daemon -> overlay 子进程, stdin 按行)
#[derive(Debug, Deserialize)]
struct OverlayEvent {
    color: String,
    ttl_ms: u64,
    rects: Vec<[i32; 4]>,
}

/// 向屏幕投递一组 OCR 识别框 (绿色, 1.2s 淡出)。`boxes` 为 os-logical [x, y, w, h]。
pub fn show_ocr_boxes(boxes: &[[i32; 4]]) {
    submit("green", 1200, boxes.iter().copied());
}

/// 标出局部截图的区域范围 (蓝色, 0.7s 淡出)。
pub fn show_capture_region(rect: [i32; 4]) {
    submit("blue", 700, std::iter::once(rect));
}

/// 点击位置闪烁 (橙色, 0.35s)。
pub fn flash_click(x: i32, y: i32) {
    submit("orange", 350, std::iter::once([x, y, 26, 26]));
}

// ----------------------------------------------------------------------------
// daemon 侧: 投递事件到 overlay 子进程
// ----------------------------------------------------------------------------

fn enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        matches!(
            std::env::var("RDOG_OVERLAY").ok().as_deref(),
            None | Some("1") | Some("true") | Some("yes")
        )
    })
}

fn submit(color: &str, ttl_ms: u64, rects: impl Iterator<Item = [i32; 4]>) {
    if !enabled() {
        return;
    }
    // 过滤退化矩形, 截断超长列表 (超长按钮列表场景)
    let rects: Vec<[i32; 4]> = rects
        .filter(|r| r[2] > 0 && r[3] > 0)
        .take(BOX_CAP)
        .collect();
    if rects.is_empty() {
        return;
    }
    let event = serde_json::json!({ "color": color, "ttl_ms": ttl_ms, "rects": rects });

    static CHILD: std::sync::OnceLock<std::sync::Mutex<Option<std::process::Child>>> =
        std::sync::OnceLock::new();
    let guard = CHILD.get_or_init(|| std::sync::Mutex::new(None));
    let mut slot = match guard.lock() {
        Ok(slot) => slot,
        Err(_) => return, // 锁毒化: 放弃可视化, 不影响主流程
    };

    // 两次尝试: 第一次写失败 (子进程已死) 则重生子进程再试一次
    for _ in 0..2 {
        // 子进程死亡 (stdin EOF 自然退出 / 崩溃) 时重生
        let need_respawn = match slot.as_mut() {
            Some(child) => child.try_wait().ok().flatten().is_some(),
            None => true,
        };
        if need_respawn {
            let exe = match std::env::current_exe() {
                Ok(exe) => exe,
                Err(_) => return,
            };
            *slot = std::process::Command::new(exe)
                .arg("overlay")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok();
        }
        let spawned = slot.is_some();
        if spawned {
            use std::io::Write;
            let child = slot.as_mut().expect("spawned");
            let stdin = child.stdin.as_mut().expect("stdin piped");
            let ok = stdin
                .write_all(event.to_string().as_bytes())
                .and_then(|_| stdin.write_all(b"\n"))
                .and_then(|_| stdin.flush())
                .is_ok();
            if ok {
                return;
            }
            // 写失败: 子进程死了, 重生重试
            *slot = None;
        }
    }
    // 可视化是尽力而为的旁路功能, 失败静默不影响控制主流程
}

// ----------------------------------------------------------------------------
// overlay 子进程主循环 (rdog overlay): 主线程跑 AppKit + stdin 事件
// ----------------------------------------------------------------------------

/// `rdog overlay` 子命令入口: 从 stdin 读 JSON 行事件并绘制。
/// stdin EOF (daemon 退出) 后自动结束。
pub fn run_overlay_main() -> io::Result<()> {
    // AppKit 事件/绘制在子进程主线程执行, MTM 合法获取 (非 unchecked)
    let mtm = objc2::MainThreadMarker::new()
        .expect("rdog overlay 子命令必须运行在进程主线程");
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Accessory);
    // 无事件循环的 helper 进程必须手动完成启动, 否则窗口订购被延迟到永远
    app.finishLaunching();

    // 主屏高度用于 os-logical (左上原点, y 向下) -> Cocoa (左下原点, y 向上)
    let main_height = objc2_app_kit::NSScreen::mainScreen(mtm)
        .map(|screen| screen.frame().size.height)
        .ok_or_else(|| io::Error::other("overlay 需要 NSScreen"))?;

    // stdin 读取线程 -> channel, 主线程事件循环消费
    let (tx, rx) = std::sync::mpsc::channel::<OverlayEvent>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            match serde_json::from_str::<OverlayEvent>(&line) {
                Ok(event) => {
                    if tx.send(event).is_err() {
                        break;
                    }
                }
                Err(_) => continue, // 坏行跳过
            }
        }
    });

    let mut active: Vec<ActivePanel> = Vec::new();
    // AppKit 的窗口订购/事务提交需要主 runloop 冲刷: 每轮手动跑一段
    // NSDefaultRunLoopMode (30ms), 否则窗口在服务器侧保持 0x0 不可见。
    let runloop = objc2_foundation::NSRunLoop::mainRunLoop();
    let default_mode: &objc2_foundation::NSRunLoopMode =
        unsafe { objc2_foundation::NSDefaultRunLoopMode };
    loop {
        // 非阻塞消化新事件; stdin EOF (daemon 退出) 则整进程退出
        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    let color = match event.color.as_str() {
                        "blue" => PanelColor::Blue,
                        "orange" => PanelColor::Orange,
                        _ => PanelColor::Green,
                    };
                    for rect in event.rects {
                        active.push(ActivePanel {
                            panel: make_panel(mtm, main_height, rect, color),
                            created: std::time::Instant::now(),
                            ttl: std::time::Duration::from_millis(event.ttl_ms),
                            base_alpha: BASE_ALPHA,
                        });
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if disconnected {
            break;
        }

        // 淡出推进 + 回收过期窗口
        let now = std::time::Instant::now();
        for panel in &mut active {
            let elapsed = now.duration_since(panel.created).as_secs_f64();
            let ttl = panel.ttl.as_secs_f64();
            // 前 60% 保持 BASE_ALPHA, 之后线性淡出
            let remain_ratio = ((ttl - elapsed) / (ttl * 0.4)).clamp(0.0, 1.0);
            panel
                .panel
                .setAlphaValue((remain_ratio * panel.base_alpha).max(0.02));
        }
        active.retain(|panel| now.duration_since(panel.created) < panel.ttl);

        // 跑一段主 runloop: 冲刷窗口事务 (让窗口服务器收到真实 frame) + 事件响应
        let limit = objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(0.03);
        runloop.runMode_beforeDate(default_mode, &limit);
    }
    Ok(())
}


struct ActivePanel {
    panel: objc2::rc::Retained<objc2_app_kit::NSWindow>,
    created: std::time::Instant,
    ttl: std::time::Duration,
    base_alpha: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PanelColor {
    Green,
    Blue,
    Orange,
}

fn make_panel(
    mtm: objc2::MainThreadMarker,
    main_height: f64,
    rect: [i32; 4],
    color: PanelColor,
) -> objc2::rc::Retained<objc2_app_kit::NSWindow> {
    let (r, g, b, alpha) = match color {
        PanelColor::Green => (0.15, 1.0, 0.35, BASE_ALPHA),
        PanelColor::Blue => (0.2, 0.55, 1.0, 0.28),
        PanelColor::Orange => (1.0, 0.55, 0.1, 0.85),
    };
    // os-logical (屏幕左上原点) -> Cocoa (主屏左下原点): cocoa_y = main_h - y - h
    let frame = objc2_foundation::NSRect::new(
        objc2_foundation::NSPoint::new(
            rect[0] as f64,
            main_height - (rect[1] as f64 + rect[3] as f64),
        ),
        objc2_foundation::NSSize::new(rect[2] as f64, rect[3] as f64),
    );
    // 必须用 NSWindow 而不是 NSPanel: 实测 (issue #102, swift 对照矩阵) macOS 对
    // NSPanel+Borderless 组合拒绝合成 (注册正确但 onscreen=false 永不上屏),
    // NSWindow+Borderless 正常合成; overlay 需要的点击穿透/置顶/全空间
    // 都是 NSWindow 原生能力, NSPanel 并非必需。
    // (objc2 将该 init 标为 unsafe, 需显式块)
    let panel = unsafe {
        objc2_app_kit::NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc::<objc2_app_kit::NSWindow>(),
            frame,
            objc2_app_kit::NSWindowStyleMask::Borderless,
            objc2_app_kit::NSBackingStoreType::Buffered,
            false,
        )
    };
    panel.setTitle(&objc2_foundation::NSString::from_str("rdog-overlay"));
    panel.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    panel.setIgnoresMouseEvents(true); // 点击穿透: overlay 永不拦截用户输入
    panel.setOpaque(false);
    panel.setHasShadow(false);
    panel.setCollectionBehavior(
        objc2_app_kit::NSWindowCollectionBehavior::CanJoinAllSpaces
            | objc2_app_kit::NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    panel.setBackgroundColor(Some(&objc2_app_kit::NSColor::colorWithCalibratedRed_green_blue_alpha(
        r, g, b, alpha,
    )));
    // 关键: 默认 contentView 没有任何可绘制内容, 窗口在服务器侧合成时是全透明
    // 的 (注册了也不可见)。给 layer 设置背景色后 Core Animation 才有真实内容。
    if let Some(content) = panel.contentView() {
        content.setWantsLayer(true);
        if let Some(layer) = content.layer() {
            layer.setBackgroundColor(Some(&objc2_core_graphics::CGColor::new_generic_rgb(
                r, g, b, alpha,
            )));
        }
    }
    panel.display();
    panel.orderFrontRegardless();
    eprintln!(
        "[overlay-diag] panel frame={frame:?} level_applied={:?} visible={:?}",
        panel.level(),
        panel.isVisible()
    );
    panel
}
