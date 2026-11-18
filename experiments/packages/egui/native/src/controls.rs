use super::*;

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
