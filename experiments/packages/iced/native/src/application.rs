//! Only the checked executable adapters (compiled against iced_app.h) see Plenty state.
use crate::{Error, Kind, Scene, build_scene};
use iced::{
    Element, Task,
    widget::{button, column, container, row, text},
};
use std::{cell::Cell, ffi::c_void, io::Write, ptr::NonNull};

unsafe extern "C" {
    #[cfg(feature = "legacy-application")]
    fn plic_app_create(error: *mut u32) -> *mut c_void;
    fn plic_app_render(state: *const c_void, token: u64, error: *mut u32) -> u32;
    fn plic_app_event(state: *mut c_void, action: u64, error: *mut u32) -> u32;
    #[cfg(feature = "legacy-application")]
    fn plic_app_destroy(state: *mut c_void);
}

fn diagnostic(message: std::fmt::Arguments<'_>) {
    // A full or closed stderr must not replace the status sent to Plenty.
    let _ = writeln!(std::io::stderr().lock(), "{message}");
}

fn checked(status: u32, tag: u32) -> Result<(), Error> {
    match (status, tag) {
        (0, _) => Ok(()),
        (1, 0) => Err(Error::OutOfMemory),
        (1, 1) => Err(Error::CapacityOverflow),
        (1, 2) => Err(Error::InvalidContext),
        (1, 3) => Err(Error::LimitExceeded),
        (1, 4) => Err(Error::InvalidLayout),
        (1, 5) => Err(Error::InvalidArgument),
        _ => Err(Error::NativeFailure),
    }
}

// NonNull is neither Send nor Sync. No callback loans survive a method return.
struct StateOwner(NonNull<c_void>);
#[cfg(feature = "legacy-application")]
impl Drop for StateOwner {
    fn drop(&mut self) {
        unsafe { plic_app_destroy(self.0.as_ptr()) }
    }
}
impl StateOwner {
    #[cfg(feature = "legacy-application")]
    fn create() -> Result<Self, Error> {
        let mut tag = 0;
        NonNull::new(unsafe { plic_app_create(&mut tag) })
            .map(Self)
            .ok_or_else(|| {
                diagnostic(format_args!("Plenty create failed: AllocError code {tag}"));
                match tag {
                    0 => Error::OutOfMemory,
                    1 => Error::CapacityOverflow,
                    _ => Error::NativeFailure,
                }
            })
    }
    fn render(&mut self) -> Result<Scene, Error> {
        build_scene(|token| {
            let mut tag = 0;
            let status = unsafe { plic_app_render(self.0.as_ptr(), token, &mut tag) };
            checked(status, tag)
        })
        .inspect_err(|e| diagnostic(format_args!("Plenty render failed: {e:?}")))
    }
    fn event(&mut self, action: u64) -> Result<(), Error> {
        let mut tag = 0;
        let status = unsafe { plic_app_event(self.0.as_ptr(), action, &mut tag) };
        checked(status, tag).inspect_err(|e| diagnostic(format_args!("Plenty event failed: {e:?}")))
    }
}

struct Session {
    state: StateOwner,
    scene: Scene,
}
impl Session {
    fn new(mut state: StateOwner) -> Result<Self, Error> {
        let scene = state.render()?;
        Ok(Self { state, scene })
    }
    fn dispatch(&mut self, event: Message) -> Result<(), Error> {
        let (action, slider) = match event {
            Message::Pressed(id) => (id, None),
            Message::Slider(id, n) => (id, Some(n)),
        };
        let valid = self
            .scene
            .nodes
            .iter()
            .any(|node| match (&node.kind, slider) {
                (Kind::Slider { id, min, max, .. }, Some(n)) => {
                    *id == action && (*min..=*max).contains(&n)
                }
                (Kind::Slider { .. }, None) => false,
                (kind, None) => kind.action() == Some(action),
                (_, Some(_)) => false,
            });
        if !valid {
            diagnostic(format_args!("unknown action ID: {action}"));
            return Err(Error::InvalidArgument);
        }
        let result = SLIDER.with(|slot| {
            slot.set(slider);
            struct Reset<'a>(&'a Cell<Option<u16>>);
            impl Drop for Reset<'_> {
                fn drop(&mut self) {
                    self.0.set(None)
                }
            }
            let _guard = Reset(slot);
            self.state.event(action)
        });
        result?;
        self.scene = self.state.render()?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
enum Message {
    Pressed(u64),
    Slider(u64, u16),
}
use Message::Pressed;
thread_local! {static SLIDER:Cell<Option<u16>>=const{Cell::new(None)};}
#[unsafe(no_mangle)]
unsafe extern "C" fn plic_v1_slider_value(out: *mut u16) -> bool {
    if let Some(value) = SLIDER.get()
        && !out.is_null()
    {
        unsafe { out.write(value) };
        true
    } else {
        false
    }
}

// `run` and iced updates share the process main thread. Report the original
// error after iced's asynchronous exit without allocating an error channel.
thread_local! { static FAILURE: Cell<Option<Error>> = const { Cell::new(None) }; }

fn update(session: &mut Session, message: Message) -> Task<Message> {
    if FAILURE.get().is_none()
        && let Err(e) = session.dispatch(message)
    {
        FAILURE.set(Some(e));
        return iced::exit();
    }
    Task::none()
}

struct Drawing<'a>(&'a Scene, &'a [usize]);
impl iced::widget::canvas::Program<Message> for Drawing<'_> {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        let mut frame = iced::widget::canvas::Frame::new(renderer, bounds.size());
        for &i in self.1 {
            let Kind::FillRect { x, y, w, h, rgb } = self.0.nodes[i].kind else {
                unreachable!("validated canvas child")
            };
            let [_, r, g, b] = rgb.to_be_bytes();
            frame.fill_rectangle(
                iced::Point::new(x.into(), y.into()),
                iced::Size::new(w.into(), h.into()),
                iced::Color::from_rgb8(r, g, b),
            );
        }
        vec![frame.into_geometry()]
    }
}
#[derive(Clone, Debug)]
pub(super) struct ComboChoice(String, u64);
impl std::fmt::Display for ComboChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
#[derive(Clone, Copy, PartialEq)]
struct Choice<'a>(&'a str, u64);
impl std::fmt::Display for Choice<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
fn view(session: &Session) -> Element<'_, Message> {
    fn element(scene: &Scene, i: usize) -> Element<'_, Message> {
        let node = &scene.nodes[i];
        let children = node.children.iter().map(|&i| element(scene, i));
        // The builder validates wrapper arity before publishing the snapshot.
        let child = |index: usize| element(scene, node.children[index]);
        match &node.kind {
            Kind::Image(handle, w, h) => iced::widget::image(handle.clone())
                .width(*w)
                .height(*h)
                .filter_method(iced::widget::image::FilterMethod::Nearest)
                .into(),
            Kind::Responsive(min) => iced::widget::responsive(move |size| {
                let items = node.children.iter().map(|&i| element(scene, i));
                if size.width >= f32::from(*min) {
                    row(items).spacing(8).into()
                } else {
                    column(items).spacing(8).into()
                }
            })
            .into(),
            Kind::ComboBox(selected, cache) => {
                let state = cache.state.get_or_init(|| {
                    let options = node
                        .children
                        .iter()
                        .map(|i| match &scene.nodes[*i].kind {
                            Kind::Choice(s, id) => ComboChoice(s.clone(), *id),
                            _ => unreachable!("validated combo-box child"),
                        })
                        .collect();
                    iced::widget::combo_box::State::new(options)
                });
                let value = state.options().iter().find(|c| c.1 == *selected);
                iced::widget::combo_box(state, "Type to filter…", value, |c| Pressed(c.1)).into()
            }
            Kind::PickList(selected) => {
                let options: Vec<_> = node
                    .children
                    .iter()
                    .map(|i| match &scene.nodes[*i].kind {
                        Kind::Choice(s, a) => Choice(s, *a),
                        _ => unreachable!("validated pick-list child"),
                    })
                    .collect();
                let value = options.iter().find(|c| c.1 == *selected).copied();
                iced::widget::pick_list(options, value, |c: Choice<'_>| Pressed(c.1))
                    .placeholder("Select…")
                    .into()
            }
            Kind::Choice(..) => unreachable!("choices render inside their list"),
            Kind::Canvas(w, h) => iced::widget::canvas(Drawing(scene, &node.children))
                .width(u32::from(*w))
                .height(u32::from(*h))
                .into(),
            Kind::FillRect { .. } => unreachable!("canvas commands render inside their canvas"),
            Kind::Slider {
                vertical,
                value,
                min,
                max,
                id,
            } => {
                let changed = move |n| Message::Slider(*id, n);
                if *vertical {
                    iced::widget::vertical_slider(*min..=*max, *value, changed)
                        .height(144)
                        .into()
                } else {
                    iced::widget::slider(*min..=*max, *value, changed).into()
                }
            }
            Kind::PaneGrid(split) => {
                use iced::widget::pane_grid::{Axis, Configuration, Content, State};
                let layout = split.layout.get_or_init(|| {
                    State::with_configuration(Configuration::Split {
                        axis: if split.side_by_side {
                            Axis::Vertical
                        } else {
                            Axis::Horizontal
                        },
                        ratio: f32::from(split.first_percent) / 100.0,
                        a: Box::new(Configuration::Pane(0)),
                        b: Box::new(Configuration::Pane(1)),
                    })
                });
                iced::widget::pane_grid(layout, |_, i, _| Content::new(child(*i)))
                    .spacing(8)
                    .into()
            }
            Kind::Grid(n) => iced::widget::grid(children).columns(usize::from(*n)).into(),
            Kind::FloatScaled(n) => iced::widget::float(child(0))
                .scale(f32::from(*n) / 100.0)
                .into(),
            Kind::Themer(dark) => iced::widget::themer(
                Some(if *dark {
                    iced::Theme::Dark
                } else {
                    iced::Theme::Light
                }),
                child(0),
            )
            .into(),
            Kind::MousePress(id) => iced::widget::mouse_area(child(0))
                .on_press(Pressed(*id))
                .into(),
            Kind::SensorShow(id) => iced::widget::sensor(child(0))
                .on_show(move |_| Pressed(*id))
                .into(),
            Kind::Container(p) => container(child(0)).padding(*p).into(),
            Kind::Pin(x, y) => iced::widget::pin(child(0))
                .x(f32::from(*x))
                .y(f32::from(*y))
                .into(),
            Kind::Scrollable(h) => iced::widget::scrollable(child(0))
                .height(u32::from(*h))
                .into(),
            Kind::Tooltip => {
                iced::widget::tooltip(child(0), child(1), iced::widget::tooltip::Position::Top)
                    .into()
            }
            Kind::Stack => iced::widget::stack(children).into(),
            Kind::Rule => iced::widget::rule::horizontal(1).into(),
            Kind::Space(w, h) => iced::widget::Space::new()
                .width(u32::from(*w))
                .height(u32::from(*h))
                .into(),
            Kind::ProgressBar(f) => iced::widget::progress_bar(0.0..=1.0, *f).into(),
            Kind::Checkbox(s, on, id) => iced::widget::checkbox(*on)
                .label(s.as_str())
                .on_toggle(move |_| Pressed(*id))
                .into(),
            Kind::Toggler(s, on, id) => iced::widget::toggler(*on)
                .label(s.as_str())
                .on_toggle(move |_| Pressed(*id))
                .into(),
            Kind::Radio(s, on, id) => {
                iced::widget::radio(s.as_str(), true, Some(*on), |_| Pressed(*id)).into()
            }
            Kind::Row => row(children).spacing(8).into(),
            Kind::Column => column(children).spacing(12).into(),
            Kind::Svg(s) => iced::widget::svg(iced::widget::svg::Handle::from_memory(
                s.as_bytes().to_vec(),
            ))
            .width(iced::Length::Shrink)
            .height(iced::Length::Shrink)
            .into(),
            Kind::Text(s) => text(s.as_str()).into(),
            Kind::Button(s, id) => button(text(s.as_str())).on_press(Pressed(*id)).into(),
        }
    }
    container(element(&session.scene, 0)).padding(24).into()
}

fn show(scene: &Scene) {
    for node in &scene.nodes {
        if let Kind::Text(s)
        | Kind::Button(s, _)
        | Kind::Checkbox(s, _, _)
        | Kind::Toggler(s, _, _)
        | Kind::Radio(s, _, _) = &node.kind
        {
            println!("{s}");
        }
    }
    println!("---");
}

fn run(create: impl FnOnce() -> Result<StateOwner, Error>) -> Result<(), Error> {
    let mut args = std::env::args_os().skip(1);
    if let Some(mode) = args.next() {
        if mode != "--headless" {
            diagnostic(format_args!("usage: plenty-iced [--headless [ACTION...]]"));
            return Err(Error::InvalidArgument);
        }
        let mut session = Session::new(create()?)?;
        show(&session.scene);
        for arg in args {
            let arg = arg.to_str().ok_or(Error::InvalidArgument)?;
            let event = if let Some((id, value)) = arg.split_once('=') {
                Message::Slider(
                    id.parse().map_err(|_| Error::InvalidArgument)?,
                    value.parse().map_err(|_| Error::InvalidArgument)?,
                )
            } else {
                Pressed(arg.parse().map_err(|_| Error::InvalidArgument)?)
            };
            session.dispatch(event)?;
            show(&session.scene);
        }
        return Ok(());
    }

    // boot is Fn, not FnOnce. Transfer the unique preflighted owner exactly once.
    let initial = Cell::new(Some(Session::new(create()?)?));
    let result = iced::application(
        move || initial.take().expect("one boot per run"),
        update,
        view,
    )
    .title("Plenty iced")
    .window_size((480.0, 280.0))
    .run()
    .map_err(|e| {
        diagnostic(format_args!("iced: {e}"));
        Error::NativeFailure
    });
    crate::loop_outcome(result, FAILURE.take())
}

#[cfg(feature = "legacy-application")]
#[unsafe(no_mangle)]
extern "C" fn plic_v1_run() -> u32 {
    status(|| {
        crate::begin_application()?;
        run(StateOwner::create)
    })
}

#[cfg(not(feature = "legacy-application"))]
#[unsafe(no_mangle)]
extern "C" fn plic_v2_admit() -> u32 {
    status(|| {
        crate::begin_application()?;
        if std::env::args_os()
            .nth(1)
            .is_some_and(|mode| mode != "--headless")
        {
            return Err(Error::InvalidArgument);
        }
        Ok(())
    })
}

#[cfg(not(feature = "legacy-application"))]
#[unsafe(no_mangle)]
extern "C" fn plic_v2_drive(driver: *mut c_void) -> u32 {
    // Borrowed only during this synchronous loop. Plenty drops its own driver.
    status(|| {
        run(|| {
            NonNull::new(driver)
                .map(StateOwner)
                .ok_or(Error::InvalidArgument)
        })
    })
}

fn status(f: impl FnOnce() -> Result<(), Error> + std::panic::UnwindSafe) -> u32 {
    std::panic::catch_unwind(f)
        .unwrap_or(Err(Error::NativeFailure))
        .err()
        .map_or(0, |e| e as u32)
}
