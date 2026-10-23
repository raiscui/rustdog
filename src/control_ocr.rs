// ============================================================================
// OCR 内容层 (specs/rdog-ocr-content-layer-plan.md v1.1)
//
// 第一引擎: oar-ocr (PaddleOCR PP-OCRv6 tiny, ONNX Runtime)。
// 模型 6MB 首跑从 ModelScope 自动下载, 按内嵌 sha256 校验, 缓存 $OAR_HOME。
//
// fail-closed 契约 (spec §5):
//   - 模型缺失 / 下载失败 / 初始化失败 / 推理超时 -> 带 reason code 的请求级失败,
//     绝不静默返回缺层 manifest (缺层会被 agent 误读成"没有文字")。
//   - reason code 走仓库 JSON 错误对象透传约定 (见 render_control_action_error_response):
//     OCR_ENGINE_UNAVAILABLE / OCR_TIMEOUT, 命名跟随 SCREENSHOT_STALE_FRAME 先例。
//
// 引擎常驻: 专用 worker 线程独占引擎实例, daemon 生命周期内复用 (模型热加载 77ms
// 只发生一次); 调用方带超时等待, 超时后 worker 仍会把过期结果投递到已关闭的
// channel (发送失败被忽略), 不会阻塞下一个请求。
// ============================================================================

use image::RgbaImage;
use serde::Serialize;
use std::io;
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// manifest ocr 层 schema (spec §4)
pub const OCR_MANIFEST_SCHEMA: &str = "rdog.ocr.v1";

/// 单次推理硬超时 (防挂死守卫, 不是性能预算)。
/// spec §7 的 warm p95 0.5s 是单窗口性能目标; 硬超时需覆盖最大的
/// 全桌面 composite 输入 (多显示器, 实测 ~1s 量级), 取 5s 留足余量。
/// 引擎构建 (模型加载/下载) 不算在这个预算里, 见 READY_TIMEOUT。
const OCR_TIMEOUT: Duration = Duration::from_secs(5);

/// 引擎初始化等待上限: 覆盖首跑模型下载 (实测 7.4s) 与冷加载。
/// spec §7 首次部署预算 30s, 这里留一倍余量。
const READY_TIMEOUT: Duration = Duration::from_secs(60);

/// 小裁剪放大阈值 (借鉴 Rust-Reader-BK98): 最短边低于此值的输入先 2x 放大
/// 再推理, 显著改善小按钮/小字号检测 (Calculator 窗口 ~200x350 实测漏检)。
const SMALL_CROP_UPSCALE_THRESHOLD: u32 = 500;

// ----------------------------------------------------------------------------
// manifest schema
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OcrManifest {
    pub schema: &'static str,
    pub engine: &'static str,
    pub language: &'static str,
    /// 文本框坐标语义: 一律 os-logical (与 screenshot manifest 坐标契约一致),
    /// agent 拿到 bbox 可直接喂 @click / @mouse-move。
    pub coordinate_space: &'static str,
    pub boxes: Vec<OcrBox>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OcrBox {
    pub index: usize,
    pub text: String,
    /// os-logical `[x, y, w, h]`, 左上原点。
    pub bbox: [i32; 4],
    /// 引擎原样透传, daemon 不过滤 (spec §4)。
    pub confidence: f32,
}

// ----------------------------------------------------------------------------
// reason code 错误
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OcrError {
    /// 模型缺失 / 下载失败 / ONNX Runtime 初始化失败 / feature 未编译
    EngineUnavailable(String),
    /// 单次识别超过 OCR_TIMEOUT
    Timeout,
}

impl OcrError {
    pub(crate) fn reason_code(&self) -> &'static str {
        match self {
            Self::EngineUnavailable(_) => "OCR_ENGINE_UNAVAILABLE",
            Self::Timeout => "OCR_TIMEOUT",
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::EngineUnavailable(_) => "ocr-unavailable",
            Self::Timeout => "ocr-timeout",
        }
    }

    fn message(&self) -> String {
        match self {
            Self::EngineUnavailable(detail) => format!("OCR 引擎不可用: {detail}"),
            Self::Timeout => "OCR 推理超时".to_owned(),
        }
    }

    fn recovery_hint(&self) -> &'static str {
        match self {
            Self::EngineUnavailable(_) => {
                "检查模型缓存目录 (默认 ~/.oar, 可用 OAR_HOME 覆盖) 与网络; 离线部署需预先布置模型文件"
            }
            Self::Timeout => "重试一次; 持续超时请缩小截图范围 (target=window) 后再试",
        }
    }

    /// 转成带 JSON 错误对象的 io::Error。
    /// 消息体是 JSON 对象时, render_control_action_error_response 会原样透传并注入 code/id,
    /// 客户端由此拿到结构化的 reason code 而不是纯文本。
    pub(crate) fn to_io_error(self) -> io::Error {
        let payload = serde_json::json!({
            "kind": self.kind(),
            "error_code": self.reason_code(),
            "error": self.message(),
            "recovery_hint": self.recovery_hint(),
        });
        io::Error::other(payload.to_string())
    }
}

impl From<OcrError> for io::Error {
    fn from(value: OcrError) -> Self {
        value.to_io_error()
    }
}

// ----------------------------------------------------------------------------
// 引擎 worker (feature 门控; 关闭 feature 时保持 fail-closed 错误路径可用)
// ----------------------------------------------------------------------------

/// worker 的一次识别任务: 图像像素坐标下的结果通过 reply 返回。
struct OcrJob {
    // feature 关闭时 worker 不消费图像, 仅保留 fail-closed 错误路径。
    #[cfg_attr(not(feature = "ocr-oar"), allow(dead_code))]
    image: image::RgbImage,
    reply: mpsc::Sender<Result<Vec<RawOcrRegion>, OcrError>>,
}

/// worker 返回的原始区域: 图像像素坐标 + 文本 + 置信度。
struct RawOcrRegion {
    text: String,
    confidence: f32,
    /// 图像像素坐标 [x, y, w, h], 左上原点
    bbox: [f32; 4],
}

static OCR_JOB_SENDER: OnceLock<mpsc::Sender<OcrJob>> = OnceLock::new();

/// 引擎就绪状态: worker 线程完成构建 (含首跑模型下载) 后设置一次。
/// Ok = 可用; Err = 构建失败 (所有后续请求 fail-closed)。
static ENGINE_READY: OnceLock<Result<(), OcrError>> = OnceLock::new();

/// 一张待识别图像 → os-logical 坐标的换算参数。
///
/// 公式与 manifest 的 `transforms.image_to_os`、
/// `ScreenshotFrame::to_os_logical_point` (src/control_mouse/screenshot_frame.rs)
/// **逐字同源**:
///     os_x = os_logical_bounds.x + (image_rect.x + image_x) * image_to_click_scale.x
///
/// 三项缺一不可, 少任何一项都会让框落到屏幕另一处:
///   - 只加原点不做缩放 = 4K@200% 下整整差 2 倍 (2026-10-06 真机缺陷);
///   - 只缩放不加 `image_offset` = 窗口裁剪图的框整体偏移一个裁剪原点。
///
/// 已知边界 (与 manifest 同一条, 不新增误差类别): 这是覆盖全桌面的线性映射,
/// 整桌面统一缩放时精确; 混合 DPI 多屏下窗口跨屏本就有近似, 逐屏精确换算需走
/// `displays[].os_logical_rect`, 而 OCR 层拿不到逐屏清单。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OcrImageSpace {
    /// 该图像像素 (0,0) 对应的 os-logical 坐标 (已含 image_offset 的平移)。
    origin: (i32, i32),
    /// 图上一个像素横跨多少 os-logical 单位 (4K@200% 下 0.5, macOS 下 1.0)。
    scale: (f64, f64),
}

impl OcrImageSpace {
    /// `os_origin`: 虚拟桌面图左上角的 os-logical 坐标。
    /// `image_offset`: 本次识别图在全虚拟桌面图内的左上角 —— 全屏/单屏图传 (0,0),
    ///   窗口裁剪图传它的 `image_rect` 原点。
    /// `scale`: manifest 的 `image_to_click_scale` (由 os_logical_bounds / 图像标尺算出)。
    pub(crate) fn new(
        os_origin: (i32, i32),
        image_offset: (i32, i32),
        scale: (f64, f64),
    ) -> Self {
        Self {
            // 浮点全程保留到末端再取整: 先按标尺折算偏移再取整, 与 to_os_logical_point
            // 的 `(image_x * scale).round()` 同语义。
            origin: (
                (f64::from(os_origin.0) + f64::from(image_offset.0) * scale.0).round() as i32,
                (f64::from(os_origin.1) + f64::from(image_offset.1) * scale.1).round() as i32,
            ),
            scale,
        }
    }

    /// 图上像素框 [x, y, w, h] → os-logical 框。宽高同样要乘标尺:
    /// 只缩放原点会让 2 倍宽的框套在真实文字的一半位置上。
    fn box_to_os_logical(self, bbox: [f32; 4]) -> [i32; 4] {
        [
            (f64::from(self.origin.0) + f64::from(bbox[0]) * self.scale.0).round() as i32,
            (f64::from(self.origin.1) + f64::from(bbox[1]) * self.scale.1).round() as i32,
            (f64::from(bbox[2]) * self.scale.0).round() as i32,
            (f64::from(bbox[3]) * self.scale.1).round() as i32,
        ]
    }
}

/// 对一张截图图像做 OCR, 产出 os-logical 坐标的 manifest ocr 层。
///
/// `space` 描述该图像像素空间到 os-logical 的换算 (`OcrImageSpace`), 两条路径
/// 各自的取法见该类型的注释。
pub(crate) fn recognize_to_manifest(
    image: &RgbaImage,
    space: OcrImageSpace,
) -> Result<OcrManifest, OcrError> {
    Ok(build_manifest(recognize_regions(image)?, space))
}

/// 引擎原始区域 → manifest ocr 层。
///
/// 与引擎解耦单独成函数: feature-off 时构造不出识别结果, 而坐标换算是本层最容易
/// 写错的部分 (2026-10-06 真机: 只平移不缩放, 框自称 os-logical 实为物理像素,
/// 坐标范围 73~2099 远超 1080)。单测直接调它即可覆盖真实换算逻辑, 不用在测试里
/// 重抄一遍公式 —— 重抄的测试与实现各错各的, 绿灯什么也证明不了。
fn build_manifest(regions: Vec<RawOcrRegion>, space: OcrImageSpace) -> OcrManifest {
    let boxes = regions
        .into_iter()
        .enumerate()
        .map(|(index, region)| OcrBox {
            index,
            text: region.text,
            confidence: region.confidence,
            bbox: space.box_to_os_logical(region.bbox),
        })
        .collect();

    OcrManifest {
        schema: OCR_MANIFEST_SCHEMA,
        engine: OCR_ENGINE_NAME,
        language: OCR_LANGUAGE,
        coordinate_space: "os-logical",
        boxes,
    }
}

/// RGBA -> RGB 并派发到 worker, 带超时等待结果。
/// 先等引擎就绪 (首次含模型下载, 宽松预算), 再等单次推理 (严格预算)。
fn recognize_regions(image: &RgbaImage) -> Result<Vec<RawOcrRegion>, OcrError> {
    wait_for_engine_ready()?;
    let rgb = rgba_to_rgb(image);
    let sender = job_sender();
    let (reply_tx, reply_rx) = mpsc::channel();
    sender
        .send(OcrJob {
            image: rgb,
            reply: reply_tx,
        })
        .map_err(|_| OcrError::EngineUnavailable("OCR worker 线程已退出".to_owned()))?;

    // 超时后 worker 仍会跑完当前任务, 只是 reply 到已弃收的 channel。
    // 串行 worker 保证下一个请求在超时任务结束后继续被处理。
    match reply_rx.recv_timeout(OCR_TIMEOUT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(OcrError::Timeout),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(OcrError::EngineUnavailable("OCR worker 线程已退出".to_owned()))
        }
    }
}

/// 等待 worker 完成引擎构建。就绪状态只设置一次, 之后所有调用零等待。
fn wait_for_engine_ready() -> Result<(), OcrError> {
    // 触发 worker 启动 (job_sender 懒初始化)
    let _ = job_sender();
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if let Some(ready) = ENGINE_READY.get() {
            return ready.clone();
        }
        if Instant::now() >= deadline {
            return Err(OcrError::EngineUnavailable(
                "OCR 引擎初始化超时 (模型下载或加载未在预算内完成)".to_owned(),
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(feature = "ocr-oar")]
const OCR_ENGINE_NAME: &str = "oar";
#[cfg(not(feature = "ocr-oar"))]
const OCR_ENGINE_NAME: &str = "unavailable";

const OCR_LANGUAGE: &str = "zh-Hans";

fn job_sender() -> &'static mpsc::Sender<OcrJob> {
    OCR_JOB_SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<OcrJob>();
        std::thread::Builder::new()
            .name("ocr-engine".to_owned())
            .spawn(move || ocr_worker_loop(rx))
            .expect("OCR worker 线程创建失败");
        tx
    })
}

#[cfg(feature = "ocr-oar")]
fn ocr_worker_loop(rx: mpsc::Receiver<OcrJob>) {
    // 引擎构建发生在循环前, 结果写入 ENGINE_READY:
    // 成功 -> 后续请求只受推理超时约束; 失败 -> 所有请求 fail-closed。
    let build = build_engine();
    let ready: Result<(), OcrError> = build.as_ref().map(|_| ()).map_err(|e| e.clone());
    let _ = ENGINE_READY.set(ready);
    let Ok(mut engine) = build else {
        return;
    };
    for job in rx {
        let result = run_recognition(&mut engine, &job.image);
        // 调用方超时弃收后 send 失败是预期行为, 忽略即可。
        let _ = job.reply.send(result);
    }
}

#[cfg(feature = "ocr-oar")]
fn build_engine() -> Result<oar_ocr::oarocr::OAROCR, OcrError> {
    // 裸模型名触发 auto-download: ModelScope + 内嵌 sha256 校验 + $OAR_HOME 缓存 (spec §8)。
    oar_ocr::oarocr::OAROCRBuilder::new(
        "pp-ocrv6_tiny_det.onnx",
        "pp-ocrv6_tiny_rec.onnx",
        "ppocrv6_tiny_dict.txt",
    )
    .build()
    .map_err(|err| OcrError::EngineUnavailable(err.to_string()))
}

#[cfg(feature = "ocr-oar")]
fn run_recognition(
    engine: &mut oar_ocr::oarocr::OAROCR,
    image: &image::RgbImage,
) -> Result<Vec<RawOcrRegion>, OcrError> {
    // 小裁剪预处理 (借鉴 Rust-Reader-BK98 实践): 小窗口 (计算器类 ~200x350)
    // 直接推理会漏检/误读小按钮, 2x 放大后再推理, bbox 等比缩回。
    // 原图足够大时不放大 (大图放大只增耗时无精度收益, 见 #96 横评)。
    let min_side = image.width().min(image.height());
    let (input, scale) = if min_side < SMALL_CROP_UPSCALE_THRESHOLD {
        let scaled = image::imageops::resize(
            image,
            image.width() * 2,
            image.height() * 2,
            image::imageops::FilterType::Triangle,
        );
        (scaled, 2.0_f32)
    } else {
        (image.clone(), 1.0)
    };

    let results = engine
        .predict(vec![input])
        .map_err(|err| OcrError::EngineUnavailable(err.to_string()))?;

    let mut regions = Vec::new();
    for result in results {
        for region in &result.text_regions {
            let Some((text, confidence)) = region.text_with_confidence() else {
                continue;
            };
            // det 检出但 rec 未认出字的区域 (text 为空) 是检测噪声:
            // 对"按文本找坐标"无价值, 在引擎边界丢弃。真实文本无论置信度高低
            // 一律透传 (spec §4: confidence 原样透传, 不做阈值过滤)。
            if text.trim().is_empty() {
                continue;
            }
            // 多边形点集 -> 轴对齐 [x, y, w, h]。BoundingBox 类型来自
            // oar-ocr 的内部依赖, 不在 rustdog 依赖树里, 因此不点名类型,
            // 直接在字段上做 min/max (坐标已转回输入图像坐标系)。
            let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
            let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
            for point in &region.bounding_box.points {
                min_x = min_x.min(point.x);
                min_y = min_y.min(point.y);
                max_x = max_x.max(point.x);
                max_y = max_y.max(point.y);
            }
            let bbox = if region.bounding_box.points.is_empty() {
                [0.0, 0.0, 0.0, 0.0]
            } else {
                // 放大图坐标 -> 原图坐标
                [
                    min_x / scale,
                    min_y / scale,
                    (max_x - min_x) / scale,
                    (max_y - min_y) / scale,
                ]
            };
            regions.push(RawOcrRegion {
                text: text.to_owned(),
                confidence,
                bbox,
            });
        }
    }
    Ok(regions)
}

#[cfg(not(feature = "ocr-oar"))]
fn ocr_worker_loop(rx: mpsc::Receiver<OcrJob>) {
    // feature 关闭: 一律 fail-closed, 不产生任何识别结果。
    for job in rx {
        let _ = job.reply.send(Err(OcrError::EngineUnavailable(
            "rdog 编译时未启用 ocr-oar feature".to_owned(),
        )));
    }
}

fn rgba_to_rgb(image: &RgbaImage) -> image::RgbImage {
    let (width, height) = (image.width(), image.height());
    let mut rgb = image::RgbImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels() {
        rgb.put_pixel(x, y, image::Rgb([pixel[0], pixel[1], pixel[2]]));
    }
    rgb
}

// ----------------------------------------------------------------------------
// 测试: 不依赖模型文件的纯逻辑部分
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_error_reason_codes_follow_screaming_snake_convention() {
        assert_eq!(
            OcrError::EngineUnavailable("模型缺失".to_owned()).reason_code(),
            "OCR_ENGINE_UNAVAILABLE"
        );
        assert_eq!(OcrError::Timeout.reason_code(), "OCR_TIMEOUT");
    }

    #[test]
    fn ocr_error_payload_is_transparent_json_object_with_reason_code() {
        // render_control_action_error_response 的透传契约: 消息体是 JSON 对象时
        // 原样透传并注入 code/id。这里锁定 reason code 字段一定在对象里。
        for err in [
            OcrError::EngineUnavailable("下载失败".to_owned()),
            OcrError::Timeout,
        ] {
            let payload = err.clone().to_io_error();
            let value: serde_json::Value =
                serde_json::from_str(&payload.to_string()).expect("错误消息必须是合法 JSON");
            assert_eq!(value["error_code"], err.reason_code());
            assert!(value["error"].is_string());
            assert!(value["recovery_hint"].is_string());
        }
    }

    #[test]
    fn image_pixels_map_into_os_logical_space_on_hidpi() {
        // 4K@200% 真机场景: 图 3840×2160 / os_logical_bounds 1920×1080 / scale 0.5。
        // 用真实采到的任务栏 OCR 框 ("MQTTX" bbox [487,1901,94,42] 同形) 做手算基准:
        //   x  = 0 + 1210 * 0.5 = 605
        //   y  = 0 + 2112 * 0.5 = 1056
        //   w  = 94  * 0.5 = 47 , h = 42 * 0.5 = 21
        // 旧实现 (只平移不缩放) 会吐出 [1210, 2112, 94, 42] —— 宽高也全错 2 倍。
        let space = OcrImageSpace::new((0, 0), (0, 0), (0.5, 0.5));
        let manifest = build_manifest(
            vec![RawOcrRegion {
                text: "MQTTX".to_owned(),
                confidence: 0.98,
                bbox: [1210.0, 2112.0, 94.0, 42.0],
            }],
            space,
        );
        assert_eq!(manifest.boxes[0].bbox, [605, 1056, 47, 21]);
        assert_eq!(manifest.schema, "rdog.ocr.v1");
        assert_eq!(manifest.coordinate_space, "os-logical");
    }

    #[test]
    fn window_crop_origin_shifts_by_image_offset_times_scale() {
        // 窗口裁剪图: 图上原点不是虚拟桌面原点, 必须先按标尺把裁剪偏移折进原点。
        //   origin = (0,0) + (image_rect.x, image_rect.y) * 0.5 = (50, 100)
        //   框 [10,20,100,30] -> (50+5, 100+10, 50, 15)
        let space = OcrImageSpace::new((0, 0), (100, 200), (0.5, 0.5));
        let manifest = build_manifest(
            vec![RawOcrRegion {
                text: "文件传输助手".to_owned(),
                confidence: 0.98,
                bbox: [10.0, 20.0, 100.0, 30.0],
            }],
            space,
        );
        assert_eq!(manifest.boxes[0].bbox, [55, 110, 50, 15]);
    }

    #[test]
    fn ocr_boxes_stay_inside_declared_os_logical_bounds() {
        // 2026-10-06 真机缺陷的判据式回归守卫: manifest 自称 os-logical 且
        // os_logical_bounds 只有 1080 高, boxes 的 y 就不该越过 1080。
        // 这里的输入是当时实测到的图上像素坐标 (y 范围 73~2099), 换成 os-logical 后
        // 必须整体落回 1920×1080。哪天又退回"只平移不缩放", y=2099 会当场炸红。
        let os_logical_bounds = (0_i32, 0_i32, 1920_i32, 1080_i32);
        let space = OcrImageSpace::new((os_logical_bounds.0, os_logical_bounds.1), (0, 0), (0.5, 0.5));
        let regions = vec![
            // 任务栏最下一行 (实测 y=2099, 旧实现直接超界 2 倍)
            RawOcrRegion {
                text: "MQTTX".to_owned(),
                confidence: 0.98,
                bbox: [487.0, 2099.0, 94.0, 42.0],
            },
            // 画面最上一行 (实测 y=73)
            RawOcrRegion {
                text: "顶部标题".to_owned(),
                confidence: 0.95,
                bbox: [100.0, 73.0, 300.0, 40.0],
            },
            // 右下角: 右/下边界允许恰好等于边界值 (图上最右像素换算后正好落在末位)
            RawOcrRegion {
                text: "右下角".to_owned(),
                confidence: 0.91,
                bbox: [3700.0, 2100.0, 140.0, 60.0],
            },
        ];
        let manifest = build_manifest(regions, space);
        assert_eq!(manifest.boxes.len(), 3);
        for box_ in &manifest.boxes {
            let [x, y, w, h] = box_.bbox;
            assert!(
                x >= os_logical_bounds.0,
                "{} 的 x={} 越出 os_logical_bounds 左界",
                box_.text,
                x
            );
            assert!(
                y >= os_logical_bounds.1,
                "{} 的 y={} 越出 os_logical_bounds 上界",
                box_.text,
                y
            );
            assert!(
                x + w <= os_logical_bounds.0 + os_logical_bounds.2,
                "{} 的右边界 x+w={} 越出 os_logical_bounds 宽度 {}",
                box_.text,
                x + w,
                os_logical_bounds.2
            );
            assert!(
                y + h <= os_logical_bounds.1 + os_logical_bounds.3,
                "{} 的下边界 y+h={} 越出 os_logical_bounds 高度 {} (今天暴露的正是这一条)",
                box_.text,
                y + h,
                os_logical_bounds.3
            );
        }
    }

    #[test]
    fn feature_off_build_fails_closed_with_engine_unavailable() {
        // 只在 feature 关闭的构建里可跑; 默认构建 (feature 开) 跳过。
        // 该测试锁定: 关 feature 后 include_ocr 必须显式失败而非静默缺层。
        #[cfg(not(feature = "ocr-oar"))]
        {
            let err = recognize_regions(&image::RgbaImage::new(4, 4))
                .err()
                .expect("feature-off 必须返回引擎不可用错误");
            assert_eq!(err.reason_code(), "OCR_ENGINE_UNAVAILABLE");
        }
    }

    /// 真机冒烟: 需要模型缓存 (OAR_HOME) 与一张输入图, 默认跳过。
    /// 本地运行: RDOG_OCR_SMOKE_IMAGE=/path/to.png OAR_HOME=/path/to/.oar cargo test --bin rdog live_engine -- --ignored
    /// CI: 由 actions/cache 预装 fixture 模型后以同名环境变量开启 (缺失即红, 不是 skip)。
    #[test]
    #[ignore = "需要 RDOG_OCR_SMOKE_IMAGE 与模型缓存 (spec §9 e2e 门控)"]
    fn live_engine_recognizes_smoke_image_when_models_available() {
        let Ok(image_path) = std::env::var("RDOG_OCR_SMOKE_IMAGE") else {
            panic!("冒烟测试需要 RDOG_OCR_SMOKE_IMAGE 指向一张待识别图像 (环境缺失应显式失败而非静默跳过)")
        };
        let image = image::open(&image_path)
            .unwrap_or_else(|err| panic!("无法读取冒烟图像 {image_path}: {err}"))
            .to_rgba8();
        // 冒烟图当作全屏图处理: 图上原点即 os-logical 原点, 无裁剪偏移;
        // scale 1.0 表示输入图已经是 os-logical 分辨率 (未知时无从判断, 取不缩放)。
        let space = OcrImageSpace::new((0, 0), (0, 0), (1.0, 1.0));
        let manifest = recognize_to_manifest(&image, space).expect("OCR 识别应成功");
        assert!(
            !manifest.boxes.is_empty(),
            "冒烟图像应识别出至少一个文本框, 实际 0 框"
        );
        for box_ in &manifest.boxes {
            assert!(!box_.text.is_empty());
            assert!((0.0..=1.0).contains(&box_.confidence));
            assert!(box_.bbox[2] > 0 && box_.bbox[3] > 0, "bbox 宽高必须为正");
        }
    }
}
