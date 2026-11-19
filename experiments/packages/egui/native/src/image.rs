use super::*;

// The strided byte-source adapter lends exactly len elements for this call.
// Copy into egui-owned pixels before returning; never retain a source pointer.
#[unsafe(no_mangle)]
unsafe extern "C" fn pleg_v1_image_rgba(
    token: u64,
    data: *const u8,
    len: usize,
    stride: usize,
    width: u16,
    height: u16,
    scale: u16,
) -> u32 {
    widget(token, |pass| {
        let size = [usize::from(width), usize::from(height)];
        if width == 0
            || height == 0
            || scale == 0
            || len != size[0] * size[1] * 4
            || data.is_null()
            || stride == 0
        {
            return Err(Error::InvalidArgument);
        }
        if width > 256
            || height > 256
            || scale > 16
            || pass.widgets == MAX_WIDGETS
            || len > MAX_PASS_BYTES - pass.bytes
        {
            return Err(Error::LimitExceeded);
        }
        (len - 1)
            .checked_mul(stride)
            .filter(|n| *n <= isize::MAX as usize)
            .ok_or(Error::CapacityOverflow)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(len / 4)
            .map_err(|_| Error::OutOfMemory)?;
        for i in (0..len).step_by(4) {
            let byte = |channel| unsafe { *data.add((i + channel) * stride) };
            pixels.push(egui::Color32::from_rgba_unmultiplied(
                byte(0usize),
                byte(1),
                byte(2),
                byte(3),
            ));
        }
        pass.bytes += len;
        pass.widgets += 1;
        let ui = pass.ui();
        let texture = ui.ctx().load_texture(
            "Plenty RGBA",
            egui::ColorImage::new(size, pixels),
            egui::TextureOptions::NEAREST,
        );
        let display = texture.size_vec2() * f32::from(scale);
        ui.add(egui::Image::from_texture(&texture).fit_to_exact_size(display));
        Ok(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strided_image_uploads_rgba_and_releases_texture_after_paint() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            with_ui(ui, |t| {
                let bytes = [128, 9, 64, 9, 32, 9, 255];
                assert_eq!(
                    unsafe { pleg_v1_image_rgba(t, bytes.as_ptr(), 4, 2, 1, 1, 2) },
                    0
                );
                Ok(())
            })
            .unwrap();
        });
        assert!(output.textures_delta.set.iter().any(|(id, deltas)| {
            output.textures_delta.free.contains(id)
                && deltas.iter().any(|delta| {
                    let egui::ImageData::Color(image) = &delta.image;
                    image.size == [1, 1] && image.pixels[0] == egui::Color32::from_rgb(128, 64, 32)
                })
        }));
        output.textures_delta.clear();
    }

    #[test]
    fn invalid_image_latches_pass_error() {
        egui::__run_test_ui(|ui| {
            let bytes = [0; 4];
            assert_eq!(
                with_ui(ui, |t| {
                    unsafe { pleg_v1_image_rgba(t, bytes.as_ptr(), 4, 1, 2, 1, 1) };
                    Ok(())
                }),
                Err(Error::InvalidArgument)
            );
        });
    }
}
