use super::super::model::{DisplayGeometry, NativeRect};
use std::ptr::{null, null_mut};
use windows_sys::Win32::{
    Foundation::{LPARAM, RECT},
    Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO},
    UI::WindowsAndMessaging::MONITORINFOF_PRIMARY,
};

const MAX_DISPLAYS: usize = 128;

pub(super) struct DisplayTopology {
    current: Vec<DisplayGeometry>,
    probe: Vec<DisplayGeometry>,
    generation: u64,
}

impl DisplayTopology {
    pub fn new() -> Self {
        Self {
            current: Vec::with_capacity(MAX_DISPLAYS),
            probe: Vec::with_capacity(MAX_DISPLAYS),
            generation: 0,
        }
    }

    pub fn observe(&mut self) -> Result<Option<(u64, Vec<DisplayGeometry>)>, String> {
        self.probe.clear();
        // SAFETY: EnumDisplayMonitors 同步调用 callback, probe 在整个枚举中独占且存活.
        let result = unsafe {
            EnumDisplayMonitors(
                null_mut(),
                null(),
                Some(collect),
                &mut self.probe as *mut Vec<DisplayGeometry> as LPARAM,
            )
        };
        if result == 0 {
            return Err("native display topology unavailable".into());
        }
        self.publish()
    }

    fn publish(&mut self) -> Result<Option<(u64, Vec<DisplayGeometry>)>, String> {
        if self.probe.is_empty() {
            return Err("recording has no display topology".into());
        }
        self.probe.sort_unstable_by_key(|display| display.id);
        if self.probe == self.current {
            return Ok(None);
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("display generation exhausted")?;
        std::mem::swap(&mut self.current, &mut self.probe);
        Ok(Some((self.generation, self.current.clone())))
    }
}

unsafe extern "system" fn collect(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> i32 {
    // SAFETY: data 指向 observe 为本次同步枚举独占的 Vec, 不跨线程或枚举保存指针.
    let displays = unsafe { &mut *(data as *mut Vec<DisplayGeometry>) };
    if displays.len() == MAX_DISPLAYS {
        return 0;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: monitor 由枚举提供, info 是 ABI 大小正确的可写缓冲区.
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return 0;
    }
    let rect = info.rcMonitor;
    let Some(width) = rect.right.checked_sub(rect.left).filter(|width| *width > 0) else {
        return 0;
    };
    let Some(height) = rect
        .bottom
        .checked_sub(rect.top)
        .filter(|height| *height > 0)
    else {
        return 0;
    };
    displays.push(DisplayGeometry {
        id: monitor as usize as u64,
        rect: NativeRect {
            x: rect.left,
            y: rect.top,
            width: width as u32,
            height: height as u32,
        },
        primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
    });
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    fn display(id: u64, x: i32) -> DisplayGeometry {
        DisplayGeometry {
            id,
            rect: NativeRect {
                x,
                y: 0,
                width: 1920,
                height: 1080,
            },
            primary: id == 1,
        }
    }

    #[test]
    fn topology_changes_publish_new_generation_but_enumeration_order_does_not() {
        let mut topology = DisplayTopology::new();
        topology.probe.extend([display(2, -1920), display(1, 0)]);
        let (generation, snapshot) = topology.publish().unwrap().unwrap();
        assert_eq!(generation, 1);
        assert_eq!(snapshot, vec![display(1, 0), display(2, -1920)]);
        topology.probe.clear();
        topology.probe.extend([display(1, 0), display(2, -1920)]);
        assert!(topology.publish().unwrap().is_none());
        topology.probe[1].rect.x = -2560;
        let (generation, changed) = topology.publish().unwrap().unwrap();
        assert_eq!(generation, 2);
        assert_eq!(changed[1].rect.x, -2560);
        assert_eq!(snapshot[1].rect.x, -1920);
    }

    #[test]
    fn missing_topology_is_not_reported_as_available() {
        assert!(DisplayTopology::new().publish().is_err());
    }
}
