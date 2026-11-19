use super::*;

// The strided byte-source adapter lends exactly len bytes until this call returns.
#[unsafe(no_mangle)]
unsafe extern "C" fn plic_v1_image_rgba(
    token: u64,
    data: *const u8,
    len: usize,
    stride: usize,
    w: u16,
    h: u16,
    scale: u16,
) -> u32 {
    command(token, |b| {
        if w == 0 || h == 0 || scale == 0 || data.is_null() || stride == 0 {
            return Err(Error::InvalidArgument);
        }
        if w > 256 || h > 256 || scale > 16 || len > MAX_FRAME_TEXT - b.text_bytes {
            return Err(Error::LimitExceeded);
        }
        if len != usize::from(w) * usize::from(h) * 4 {
            return Err(Error::InvalidArgument);
        }
        (len - 1)
            .checked_mul(stride)
            .filter(|n| *n <= isize::MAX as usize)
            .ok_or(Error::CapacityOverflow)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(len)
            .map_err(|_| Error::OutOfMemory)?;
        for i in 0..len {
            pixels.push(unsafe { *data.add(i * stride) })
        }
        let (w, h, s) = (u32::from(w), u32::from(h), u32::from(scale));
        let handle = iced::widget::image::Handle::from_rgba(w, h, pixels);
        b.add(Node {
            kind: Kind::Image(handle, w * s, h * s),
            children: vec![],
        })?;
        b.text_bytes += len;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_owns_strided_pixels() {
        let mut bytes = [1, 0, 2, 0, 3, 0, 255];
        let scene = build_scene(|t| {
            assert_eq!(
                unsafe { plic_v1_image_rgba(t, bytes.as_ptr(), 4, 2, 1, 1, 4) },
                0
            );
            bytes.fill(0);
            Ok(())
        })
        .unwrap();
        assert!(
            matches!(&scene.nodes[0].kind, Kind::Image(iced::widget::image::Handle::Rgba{pixels,..},4,4) if pixels.as_ref()==[1,2,3,255])
        );
    }
    #[test]
    fn malformed_image_discards_snapshot() {
        assert_eq!(
            build_scene(|t| {
                unsafe { plic_v1_image_rgba(t, [0; 4].as_ptr(), 4, 1, 2, 1, 1) };
                Ok(())
            }),
            Err(Error::InvalidArgument)
        );
    }
}
