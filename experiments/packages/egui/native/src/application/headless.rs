//! Display-free driver for the *same* egui pass and Plenty owner. Arguments
//! are logical pointer coordinates, not widget names, ordinals or actions.
use super::{Error, Owner, context};
use egui::{Event, Modifiers, PointerButton, RawInput, Rect, Shape, pos2, vec2};
use std::{
    ffi::OsString,
    io::{self, Write},
};

fn painted_text(shape: &Shape, out: &mut impl Write) -> io::Result<()> {
    match shape {
        Shape::Text(t) => writeln!(out, "{}", t.galley.text()),
        Shape::Vec(v) => v.iter().try_for_each(|s| painted_text(s, out)),
        _ => Ok(()),
    }
}

pub(super) fn run(mut state: Owner, clicks: impl Iterator<Item = OsString>) -> Result<(), Error> {
    let ctx = context();
    let mut frame = |events| {
        let input = RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(pos2(0., 0.), vec2(480., 280.))),
            ..Default::default()
        };
        let mut result = Ok(());
        let mut output = ctx.run_ui(input, |ui| {
            result = state.draw(ui);
            // Exercise a request that could otherwise cause multiple UI calls.
            ui.ctx().request_discard("single-pass input test");
        });
        output.textures_delta.clear(); // No renderer consumes this headless frame.
        result?;
        if output.platform_output.num_completed_passes != 1 {
            return Err(Error::NativeFailure);
        }
        let mut out = io::stdout().lock();
        for shape in output.shapes {
            // Tracing is optional. A full diagnostic sink must not prevent the
            // next real input (or mask its checked callback error).
            if painted_text(&shape.shape, &mut out).is_err() {
                break;
            }
        }
        Ok(())
    };
    frame(vec![])?;
    for (index, click) in clicks.enumerate() {
        if index >= 64 {
            return Err(Error::LimitExceeded);
        }
        let (x, y) = click
            .to_str()
            .and_then(|s| s.split_once(','))
            .ok_or(Error::InvalidArgument)?;
        let coord = |s: &str| {
            s.parse::<u16>()
                .map(f32::from)
                .map_err(|_| Error::InvalidArgument)
        };
        let pos = pos2(coord(x)?, coord(y)?);
        frame(vec![Event::PointerMoved(pos)])?;
        for pressed in [true, false] {
            frame(vec![Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }])?;
        }
        for _ in 0..4 {
            frame(vec![])?;
        }
    }
    Ok(())
}
