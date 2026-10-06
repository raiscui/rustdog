//! 截图坐标系缓存 — 让 `@click` 能直接吃"图上像素坐标"
//!
//! # 为什么需要它
//!
//! 2026-10-05 Windows 真机 (4K@200%) 实战: `@screenshot` 给的是 3840×2160 的图,
//! `@click` 吃的是 1920×1080 的 os-logical 坐标, 两边差一个 scale。manifest 早就把
//! `os_logical_bounds` / `image_to_click_scale` 算出来发给用户了, 但 agent 要么算错
//! (连续三次点空, 浪费约 20 分钟), 要么根本不知道该拿哪两个数去乘。
//!
//! 只把公式发给 agent 让它自己算, 是把同一个坑从"没标尺"换成"要心算"。真正省事的
//! 形态是 daemon 侧记住每次截图的坐标系, 让 `@click:{screenshot:"<id>", x:1104, y:213}`
//! 里的 x/y 就是**图上像素**: agent 只需报出它在图上看到的位置, 换算由 manifest 那套
//! 公式 (唯一真相源) 在 daemon 内完成。
//!
//! # 换算公式 (与 manifest 的 `transforms.image_to_os` 逐字同源)
//!
//! `os_x = os_logical_bounds.x + (image_rect.x + image_x) * image_to_click_scale.x`
//!
//! 全屏 / 单屏图的 `image_rect` 是 (0, 0, 图宽, 图高); 窗口裁剪图的 `image_rect` 是这张
//! 裁剪图在全虚拟桌面图内的位置 —— 少了这一项平移, 窗口截图上的点会整体偏移一个窗口原点。
//!
//! 已知边界 (与 manifest 同一条): 整桌面统一缩放时精确; 混合 DPI 多屏下应改走
//! `displays[].os_logical_rect` 逐屏换算, 窗口跨屏时本近似会有偏差。

use super::request::MousePoint;
use crate::control_display_scope::DisplayRect;
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

/// 缓存有效期: 用户从"拿到截图"到"决定点哪"通常在几十秒到几分钟内, 10 分钟够覆盖
/// "看图 → 想 → 点"的完整往返; 又短到 daemon 不会拿着半小时前的画面做换算 —— 那时
/// 的缩放 / 显示器布局多半已经不成立, 换算对了也是点错地方。
pub const SCREENSHOT_FRAME_TTL_MS: u64 = 10 * 60 * 1000;

/// 容量上限: 一轮远程排障大约看十几张图, 32 条足够覆盖一个工作窗口; 再往上基本是
/// agent 在刷截图, 旧坐标系留着没有价值。
pub const SCREENSHOT_FRAME_CACHE_CAPACITY: usize = 32;

/// 图上 1 像素 = 多少 os-logical 单位 (4K@200% 下 0.5), 方向 image→click。
///
/// 名字带 `Screenshot` 前缀是为了与 `screenshot.rs` 里那个只用于序列化的私有同名
/// struct 区分开 —— 写侧调用方不必 (也不应该) 把两者混为一个类型。
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ScreenshotImageToClickScale {
    pub x: f64,
    pub y: f64,
}

/// 一次截图的坐标系快照: 记住"这张图的像素该怎么换算成 os-logical"。
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenshotFrame {
    pub screenshot_id: String,
    /// 虚拟桌面在 os-logical 空间的边界, 换算原点与标尺都来自它。
    pub os_logical_bounds: DisplayRect,
    /// 本次给用户看的图在**全虚拟桌面图**内的位置。`width` / `height` 即 manifest 的
    /// `image_size` (本次图的像素尺寸), `x` / `y` 是窗口裁剪图才有的平移量。
    pub image_rect: DisplayRect,
    pub image_to_click_scale: ScreenshotImageToClickScale,
}

impl ScreenshotFrame {
    /// 全虚拟桌面图 / 单屏图: 图上原点就是虚拟桌面左上角, 没有平移项。
    #[allow(dead_code)] // 写侧接线在 src/screenshot.rs (manifest 构造处), 见模块头注
    pub fn composite(
        screenshot_id: &str,
        image_width: u32,
        image_height: u32,
        os_logical_bounds: DisplayRect,
        image_to_click_scale: ScreenshotImageToClickScale,
    ) -> Self {
        Self {
            screenshot_id: screenshot_id.to_owned(),
            os_logical_bounds,
            image_rect: DisplayRect {
                x: 0,
                y: 0,
                width: image_width,
                height: image_height,
            },
            image_to_click_scale,
        }
    }

    /// 窗口裁剪图: 图上原点不等于虚拟桌面原点, 换算时必须先加 `image_rect` 平移。
    #[allow(dead_code)] // 同 composite: 写侧接线在 src/screenshot.rs
    pub fn window(
        screenshot_id: &str,
        image_rect: DisplayRect,
        os_logical_bounds: DisplayRect,
        image_to_click_scale: ScreenshotImageToClickScale,
    ) -> Self {
        Self {
            screenshot_id: screenshot_id.to_owned(),
            os_logical_bounds,
            image_rect,
            image_to_click_scale,
        }
    }

    /// 本次图的像素尺寸 (即 manifest 的 `image_size`)。
    pub fn image_size(&self) -> (u32, u32) {
        (self.image_rect.width, self.image_rect.height)
    }

    /// 图上像素是否落在这张图内。
    ///
    /// 调用方给的是**相对本图左上角**的像素, 所以判据是 `[0, 图宽)`, 不是
    /// `image_rect` 自身的范围 —— 窗口图按后者判会把裁剪偏移当成图内区域。
    pub fn contains_image_point(&self, point: MousePoint) -> bool {
        let (Ok(x), Ok(y)) = (u32::try_from(point.x), u32::try_from(point.y)) else {
            return false;
        };
        x < self.image_rect.width && y < self.image_rect.height
    }

    /// 图上像素 → os-logical 点 (`@click` 接受的坐标空间)。
    ///
    /// 浮点转整数用 `as` (Rust 的 float→int cast 是饱和语义, 不会 UB); 舍入用
    /// `round` (半远离零), 与 manifest 公式的隐含语义一致。
    pub fn to_os_logical_point(&self, point: MousePoint) -> MousePoint {
        let image_x = f64::from(self.image_rect.x) + f64::from(point.x);
        let image_y = f64::from(self.image_rect.y) + f64::from(point.y);
        MousePoint {
            x: self.os_logical_bounds.x + (image_x * self.image_to_click_scale.x).round() as i32,
            y: self.os_logical_bounds.y + (image_y * self.image_to_click_scale.y).round() as i32,
        }
    }
}

struct CachedFrame {
    frame: ScreenshotFrame,
    stored_at_unix_ms: u64,
}

static SCREENSHOT_FRAME_STORE: OnceLock<Mutex<VecDeque<CachedFrame>>> = OnceLock::new();

fn with_frame_cache<T>(run: impl FnOnce(&mut VecDeque<CachedFrame>) -> T) -> T {
    let store = SCREENSHOT_FRAME_STORE.get_or_init(|| Mutex::new(VecDeque::new()));
    // 缓存不可用不该让点击失败: 锁中毒时取回内层继续用, 最坏结果只是回一句
    // "截图已过期", 比整条 mouse 链崩掉好。
    let mut guard = store.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    run(&mut guard)
}

/// 登记一次截图的坐标系。**写侧调用点在 `src/screenshot.rs`** (manifest 构造处):
/// 本模块只提供存储与换算, 那里每产出一次 manifest 就要登记一次
/// (`ScreenshotFrame::composite` / `::window` + 本函数), 否则 `@click:{screenshot:..}`
/// 永远查不到东西。
#[allow(dead_code)]
pub fn remember_screenshot_frame(frame: ScreenshotFrame) {
    remember_screenshot_frame_at(frame, now_unix_epoch_millis());
}

#[allow(dead_code)]
pub(crate) fn remember_screenshot_frame_at(frame: ScreenshotFrame, now_unix_ms: u64) {
    with_frame_cache(|cache| {
        // 同一个 id 重新登记 = 又截了一次 (id 是毫秒时间戳): 丢掉旧条目让新条目排到
        // 队尾, TTL 与淘汰顺序都按"最后一次登记"算。
        cache.retain(|cached| {
            cached.frame.screenshot_id != frame.screenshot_id
                && !is_expired(cached.stored_at_unix_ms, now_unix_ms)
        });
        cache.push_back(CachedFrame {
            frame,
            stored_at_unix_ms: now_unix_ms,
        });
        while cache.len() > SCREENSHOT_FRAME_CACHE_CAPACITY {
            cache.pop_front();
        }
    });
}

/// 按 `screenshot_id` 回查坐标系; 不存在或已过期都返回 `None`
/// (调用方据此回"请重新截图")。
pub fn lookup_screenshot_frame(screenshot_id: &str) -> Option<ScreenshotFrame> {
    lookup_screenshot_frame_at(screenshot_id, now_unix_epoch_millis())
}

pub(crate) fn lookup_screenshot_frame_at(
    screenshot_id: &str,
    now_unix_ms: u64,
) -> Option<ScreenshotFrame> {
    with_frame_cache(|cache| {
        let index = cache
            .iter()
            .position(|cached| cached.frame.screenshot_id == screenshot_id)?;
        if is_expired(cache[index].stored_at_unix_ms, now_unix_ms) {
            // 过期即删: 留着只会让同 id 的下一次查询继续走同一条死路, 还白占容量。
            cache.remove(index);
            return None;
        }
        Some(cache[index].frame.clone())
    })
}

fn is_expired(stored_at_unix_ms: u64, now_unix_ms: u64) -> bool {
    // saturating_sub: 系统时钟回拨时差值取 0 (判为未过期), 好过因无符号回绕把
    // 所有条目一次性判死。
    now_unix_ms.saturating_sub(stored_at_unix_ms) >= SCREENSHOT_FRAME_TTL_MS
}

fn now_unix_epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
        .unwrap_or_default()
}

/// 缓存是进程级单例, 并行单测会互相淘汰条目。用一把测试锁把涉及缓存的用例串起来,
/// 免得"这个用例刚登记的 id 被另一个用例的 33 条插入挤掉"这种顺序相关的假红。
#[cfg(test)]
pub(crate) fn with_frame_cache_test_guard<T>(run: impl FnOnce() -> T) -> T {
    static FRAME_CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());
    let _guard = FRAME_CACHE_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    run()
}

#[cfg(test)]
pub(crate) fn clear_frame_cache_for_test() {
    with_frame_cache(|cache| cache.clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(x: i32, y: i32, width: u32, height: u32) -> DisplayRect {
        DisplayRect {
            x,
            y,
            width,
            height,
        }
    }

    fn scale(x: f64, y: f64) -> ScreenshotImageToClickScale {
        ScreenshotImageToClickScale { x, y }
    }

    /// 4K@200% 单屏全屏图: 图上 3840×2160 ↔ os-logical 1920×1080, 标尺 0.5。
    /// 这正是 2026-10-05 现场点空三次的那块屏。
    #[test]
    fn composite_frame_maps_image_pixel_to_os_logical() {
        let frame = ScreenshotFrame::composite(
            "composite-scale",
            3840,
            2160,
            bounds(0, 0, 1920, 1080),
            scale(0.5, 0.5),
        );

        assert_eq!(
            frame.to_os_logical_point(MousePoint { x: 1104, y: 212 }),
            MousePoint { x: 552, y: 106 }
        );
    }

    /// 舍入取整: 0.5 落在整数边界上时半远离零, 差一个像素就是点偏半个逻辑单位。
    #[test]
    fn composite_frame_rounds_half_away_from_zero() {
        let frame = ScreenshotFrame::composite(
            "composite-round",
            3840,
            2160,
            bounds(0, 0, 1920, 1080),
            scale(0.5, 0.5),
        );

        assert_eq!(
            frame.to_os_logical_point(MousePoint { x: 1104, y: 213 }),
            MousePoint { x: 552, y: 107 }
        );
    }

    /// 窗口裁剪图: 图上原点不等于虚拟桌面原点, 少加 image_rect 平移整张图会偏出去。
    #[test]
    fn window_frame_applies_image_origin_offset() {
        let frame = ScreenshotFrame::window(
            "window-origin",
            bounds(800, 600, 1280, 720),
            bounds(0, 0, 1920, 1080),
            scale(0.5, 0.5),
        );

        assert_eq!(
            frame.to_os_logical_point(MousePoint { x: 100, y: 50 }),
            MousePoint { x: 450, y: 325 }
        );
    }

    /// 越界像素必须被挡住: 放行等于把"图上看着在这儿"换算成屏幕上一个无关的点。
    #[test]
    fn image_point_outside_image_is_rejected() {
        let frame = ScreenshotFrame::composite(
            "bounds-check",
            3840,
            2160,
            bounds(0, 0, 1920, 1080),
            scale(0.5, 0.5),
        );

        assert!(frame.contains_image_point(MousePoint { x: 0, y: 0 }));
        assert!(frame.contains_image_point(MousePoint {
            x: 3839,
            y: 2159
        }));
        assert!(!frame.contains_image_point(MousePoint { x: 3840, y: 0 }));
        assert!(!frame.contains_image_point(MousePoint { x: 0, y: -1 }));
        assert!(!frame.contains_image_point(MousePoint {
            x: i32::MIN,
            y: i32::MIN
        }));
    }

    fn frame(id: &str) -> ScreenshotFrame {
        ScreenshotFrame::composite(
            id,
            3840,
            2160,
            bounds(0, 0, 1920, 1080),
            scale(0.5, 0.5),
        )
    }

    #[test]
    fn cached_frame_is_readable_within_ttl() {
        with_frame_cache_test_guard(|| {
            clear_frame_cache_for_test();
            remember_screenshot_frame_at(frame("ttl-alive"), 1_000);

            let found =
                lookup_screenshot_frame_at("ttl-alive", 1_000 + SCREENSHOT_FRAME_TTL_MS - 1);

            assert_eq!(found, Some(frame("ttl-alive")));
        });
    }

    /// TTL 到点即失效: 换算依据是截图当时的缩放/布局, 拖久了只会点错地方。
    #[test]
    fn cached_frame_expires_after_ttl() {
        with_frame_cache_test_guard(|| {
            clear_frame_cache_for_test();
            remember_screenshot_frame_at(frame("ttl-dead"), 1_000);

            let found = lookup_screenshot_frame_at("ttl-dead", 1_000 + SCREENSHOT_FRAME_TTL_MS);

            assert_eq!(found, None);
        });
    }

    /// 过期条目顺手清掉, 别让它占着容量把还新鲜的截图挤出去。
    #[test]
    fn expired_entry_is_dropped_on_lookup() {
        with_frame_cache_test_guard(|| {
            clear_frame_cache_for_test();
            remember_screenshot_frame_at(frame("ttl-purge"), 1_000);
            assert!(
                lookup_screenshot_frame_at("ttl-purge", 1_000 + SCREENSHOT_FRAME_TTL_MS)
                    .is_none()
            );

            let remaining = with_frame_cache(|cache| cache.len());

            assert_eq!(remaining, 0);
        });
    }

    #[test]
    fn capacity_evicts_in_insertion_order() {
        with_frame_cache_test_guard(|| {
            clear_frame_cache_for_test();
            for index in 0..=SCREENSHOT_FRAME_CACHE_CAPACITY {
                remember_screenshot_frame_at(frame(&format!("cap-{index}")), 1_000);
            }

            assert!(lookup_screenshot_frame_at("cap-0", 1_000).is_none());
            assert!(lookup_screenshot_frame_at("cap-1", 1_000).is_some());
            assert!(
                lookup_screenshot_frame_at(
                    &format!("cap-{SCREENSHOT_FRAME_CACHE_CAPACITY}"),
                    1_000
                )
                .is_some()
            );
        });
    }

    /// 同一 id 再截一次: 旧的直接顶掉, 不留两条互相矛盾的坐标系。
    #[test]
    fn re_registering_same_id_replaces_and_refreshes() {
        with_frame_cache_test_guard(|| {
            clear_frame_cache_for_test();
            remember_screenshot_frame_at(frame("re-register"), 1_000);
            let refreshed = ScreenshotFrame::composite(
                "re-register",
                1920,
                1080,
                bounds(0, 0, 1920, 1080),
                scale(1.0, 1.0),
            );
            remember_screenshot_frame_at(refreshed.clone(), 1_000 + SCREENSHOT_FRAME_TTL_MS - 1);

            // 查点落在"重新登记 + 差 1ms": TTL 判定是 >=, 所以这里必须留出 1ms 余量。
            let found = lookup_screenshot_frame_at(
                "re-register",
                1_000 + 2 * SCREENSHOT_FRAME_TTL_MS - 2,
            );

            assert_eq!(found, Some(refreshed));
        });
    }
}
