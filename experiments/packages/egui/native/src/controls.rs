use super::*;

#[unsafe(no_mangle)]
unsafe extern "C" fn pleg_v1_range_slider(
    token: u64,
    low: *mut f32,
    high: *mut f32,
    min: f32,
    max: f32,
) -> u32 {
    control(token, |ui| {
        // The Plenty facade supplies two exclusive call-scoped loans.
        if low.is_null() || high.is_null() || low == high {
            return Err(Error::InvalidArgument);
        }
        let (mut l, mut h) = unsafe { (*low, *high) };
        let valid =
            |l: f32, h: f32| min.is_finite() && max.is_finite() && min <= l && l <= h && h <= max;
        if !valid(l, h) {
            return Err(Error::InvalidArgument);
        }
        let changed = ui
            .add(egui::RangeSlider::new(&mut l, &mut h, min..=max))
            .changed();
        if !valid(l, h) {
            return Err(Error::InvalidArgument);
        }
        unsafe {
            *low = l;
            *high = h;
        }
        Ok(changed)
    })
}

// Keep scalar drafts local: no change reaches the model on a checked error.
#[unsafe(no_mangle)]
unsafe extern "C" fn pleg_v1_checkbox(
    token: u64,
    p: *const u8,
    len: usize,
    value: *mut bool,
) -> u32 {
    unsafe {
        text_widget(token, p, len, |pass, text| {
            let slot = value.as_mut().ok_or(Error::InvalidArgument)?;
            let mut draft = *slot;
            let changed = pass.ui().checkbox(&mut draft, text).changed();
            *slot = draft;
            Ok(changed)
        })
    }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn pleg_v1_radio(token: u64, p: *const u8, len: usize, selected: bool) -> u32 {
    unsafe {
        text_widget(token, p, len, |pass, text| {
            Ok(pass.ui().radio(selected, text).clicked())
        })
    }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn pleg_v1_link(token: u64, p: *const u8, len: usize) -> u32 {
    unsafe {
        text_widget(token, p, len, |pass, text| {
            Ok(pass.ui().link(text).clicked())
        })
    }
}

// Accept only absolute web URLs, never file paths or custom protocol handlers.
fn web_url(text: &str) -> bool {
    !text
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
        && (text.starts_with("https://") || text.starts_with("http://"))
        && url::Url::parse(text)
            .is_ok_and(|u| u.has_host() && u.username().is_empty() && u.password().is_none())
}
#[unsafe(no_mangle)]
unsafe extern "C" fn pleg_v1_hyperlink(token: u64, p: *const u8, len: usize) -> u32 {
    unsafe {
        text_widget(token, p, len, |pass, url| {
            if !web_url(url) {
                return Err(Error::InvalidArgument);
            }
            Ok(pass.ui().hyperlink(url).clicked())
        })
    }
}
fn control(token: u64, draw: impl FnOnce(&mut egui::Ui) -> Result<bool, Error>) -> u32 {
    widget(token, |pass| {
        if pass.widgets == MAX_WIDGETS {
            return Err(Error::LimitExceeded);
        }
        pass.widgets += 1;
        draw(pass.ui())
    })
}
#[unsafe(no_mangle)]
extern "C" fn pleg_v1_separator(token: u64) -> u32 {
    control(token, |ui| {
        ui.separator();
        Ok(false)
    })
}
#[unsafe(no_mangle)]
extern "C" fn pleg_v1_spinner(token: u64) -> u32 {
    control(token, |ui| {
        ui.spinner();
        Ok(false)
    })
}
#[unsafe(no_mangle)]
extern "C" fn pleg_v1_progress_bar(token: u64, fraction: f32) -> u32 {
    control(token, |ui| {
        if !(0.0..=1.0).contains(&fraction) {
            return Err(Error::InvalidArgument);
        }
        ui.add(egui::ProgressBar::new(fraction));
        Ok(false)
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn pleg_v1_slider(token: u64, value: *mut f32, min: f32, max: f32) -> u32 {
    control(token, |ui| {
        let slot = unsafe { value.as_mut() }.ok_or(Error::InvalidArgument)?;
        if !min.is_finite() || !max.is_finite() || !(min..=max).contains(slot) {
            return Err(Error::InvalidArgument);
        }
        let mut draft = *slot;
        let changed = ui.add(egui::Slider::new(&mut draft, min..=max)).changed();
        if !(min..=max).contains(&draft) {
            return Err(Error::InvalidArgument);
        }
        *slot = draft;
        Ok(changed)
    })
}
#[unsafe(no_mangle)]
unsafe extern "C" fn pleg_v1_drag_value(token: u64, value: *mut f32, min: f32, max: f32) -> u32 {
    control(token, |ui| {
        let slot = unsafe { value.as_mut() }.ok_or(Error::InvalidArgument)?;
        if !min.is_finite() || !max.is_finite() || !(min..=max).contains(slot) {
            return Err(Error::InvalidArgument);
        }
        let mut draft = *slot;
        let changed = ui
            .add(egui::DragValue::new(&mut draft).range(min..=max))
            .changed();
        if !(min..=max).contains(&draft) {
            return Err(Error::InvalidArgument);
        }
        *slot = draft;
        Ok(changed)
    })
}

#[cfg(test)]
mod range_tests {
    use super::*;
    #[test]
    fn web_links_only() {
        assert!(web_url("https://example.org/manual?q=2"));
        for url in [
            "file:///etc",
            "example.org",
            "https://user@example.org/",
            "javascript:alert(1)",
            "https://example.org/\n",
        ] {
            assert!(!web_url(url));
        }
    }
    #[test]
    fn invalid_endpoints_latch_without_changing_either_value() {
        egui::__run_test_ui(|ui| {
            let (mut low, mut high) = (0.8, 0.2);
            let result = with_ui(ui, |t| {
                assert_eq!(
                    unsafe { pleg_v1_range_slider(t, &mut low, &mut high, 0.0, 1.0) },
                    Error::InvalidArgument as u32
                );
                Ok(())
            });
            assert_eq!(result, Err(Error::InvalidArgument));
            assert_eq!((low, high), (0.8, 0.2));
        });
    }
}
