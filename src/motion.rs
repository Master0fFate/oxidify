use std::time::{Duration, Instant};

const RECHECK: Duration = Duration::from_secs(1);

pub const ANIMATION_TIME: f32 = 0.12;

#[derive(Default)]
pub struct Motion {
    reduced: bool,
    checked: Option<Instant>,
}

impl Motion {
    pub fn reduced(&self) -> bool {
        self.reduced
    }

    pub fn refresh(&mut self, now: Instant) -> bool {
        if self
            .checked
            .is_some_and(|checked| now.duration_since(checked) < RECHECK)
        {
            return false;
        }
        self.checked = Some(now);
        let reduced = system_reduced();
        let changed = self.reduced != reduced;
        self.reduced = reduced;
        changed
    }
}

pub fn apply(ctx: &egui::Context, reduced: bool) {
    let time = if reduced { 0.0 } else { ANIMATION_TIME };
    let scroll = if reduced {
        egui::style::ScrollAnimation::none()
    } else {
        egui::style::ScrollAnimation::default()
    };
    ctx.all_styles_mut(|style| {
        style.animation_time = time;
        style.scroll_animation = scroll;
    });
    ctx.data_mut(|data| data.insert_temp(reduced_id(), reduced));
}

fn reduced_id() -> egui::Id {
    egui::Id::new("oxidify-reduced-motion")
}

pub fn seconds(ctx: &egui::Context, seconds: f32) -> f32 {
    if ctx
        .data(|data| data.get_temp(reduced_id()))
        .unwrap_or(false)
    {
        0.0
    } else {
        seconds
    }
}

#[cfg(windows)]
fn system_reduced() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
    };
    let mut enabled: windows_sys::core::BOOL = 1;
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut enabled as *mut windows_sys::core::BOOL).cast(),
            0,
        )
    };
    read != 0 && enabled == 0
}

#[cfg(target_os = "macos")]
fn system_reduced() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn system_reduced() -> bool {
    false
}
