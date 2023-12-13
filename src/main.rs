#![feature(array_chunks, iter_array_chunks, slice_flatten, iter_intersperse)]

mod config;
mod font;
mod state;

use battery::Manager;
use config::configure;
use pixels::wgpu::BlendState;
use state::State;

use pixels::{PixelsBuilder, SurfaceTexture};
use sysinfo::{System, SystemExt};
use winit::dpi::{LogicalPosition, PhysicalSize};
use winit::event::Event;
use winit::event_loop::{ControlFlow, EventLoop};
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
use winit::platform::x11::{WindowBuilderExtX11, XWindowType};
use winit::window::{Window, WindowBuilder, WindowLevel};
use winit_input_helper::WinitInputHelper;
#[cfg(target_os = "macos")]
use {
    cocoa::appkit::{NSWindow, NSWindowCollectionBehavior},
    objc::runtime::{objc_release, objc_retain, Object},
    winit::platform::macos::WindowExtMacOS,
};

const WINDOW_NAME: &str = env!("CARGO_BIN_NAME");

fn setup_window(
    size: PhysicalSize<u32>,
    position: LogicalPosition<u32>,
    event_loop: &EventLoop<()>,
) -> Window {
    let builder = WindowBuilder::new()
        .with_active(false)
        .with_decorations(false)
        .with_resizable(false)
        .with_transparent(true)
        .with_title(WINDOW_NAME)
        .with_inner_size(size)
        .with_max_inner_size(size)
        .with_min_inner_size(size)
        .with_position(position)
        .with_window_level(WindowLevel::AlwaysOnTop);

    // On Linux (and BSDs, which I have not been able to test), Wayland and X11 are supported. On
    // these platforms, we can set a name. On X11 specifically, we want to set the window type to
    // Dock, which means it is properly treated as an immovable bar.
    //
    // Thanks to Maya.
    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
    let builder = builder
        .with_base_size(size)
        .with_name(WINDOW_NAME, WINDOW_NAME)
        .with_x11_window_type(vec![XWindowType::Dock]);

    let window = builder.build(event_loop).unwrap();
    #[cfg(target_os = "macos")]
    make_window_sticky_on_mac(&window);
    window
}

#[cfg(target_os = "macos")]
fn make_window_sticky_on_mac(window: &Window) {
    let mac_window = window as &dyn WindowExtMacOS;
    let ns_window_id = mac_window.ns_window();
    // Safety: ns_window_id points to a valid NSWindow Object and is non-null.
    unsafe {
        let ns_window: *mut Object = std::mem::transmute(ns_window_id);
        objc_retain(ns_window);
        ns_window.setCollectionBehavior_(
            NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces,
        );
        objc_release(ns_window);
    }
}

fn main() -> Result<(), pixels::Error> {
    let config = match configure() {
        Ok(args) => args,
        Err(err) => {
            eprintln!("ERROR: {err}");
            eprintln!("Run with --help for usage information.");
            std::process::exit(1);
        }
    };

    let font = font::load_font(&config.font_path);
    let mut state = State::new(
        font,
        System::new(),
        Manager::new().map_or(None, |m| match m.batteries() {
            Ok(mut bats) => bats.next().and_then(|err| err.ok()),
            Err(_) => None,
        }),
        mpd::Client::connect(config.mpd_addr).ok(),
        config.foreground,
        config.background,
        config.elements,
    );

    let event_loop = EventLoop::new();

    // In order to deal well with higher resolution displays, fractional scale factors (e.g., 1.67)
    // are quantized to integers.
    //
    // We can get the scale factor from the window, but in order to create the window, we must
    // first know the scale factor. To circumvent that circular mess, we can create a dummy window,
    // read its scale factor, and use that to eventually set up our actual window. The dummy window
    // is dropped immediately after its created.
    // Note that this method relies on the assumption that both times we create a winit::Window, the
    // monitor it picks (and its scale factor) will be the same.
    //
    // If the environment variable is not set, the scale factor is determined with the dummy window
    // method.
    let scale_factor = {
        let env_scale_factor = std::env::var("TID_SCALE_FACTOR")
            .ok()
            .and_then(|v| v.parse::<f64>().ok().map(|f| u32::max(1, f.round() as u32)));
        let wm_scale_factor = || {
            let dummy = Window::new(&event_loop).unwrap();
            dummy.scale_factor()
        };
        env_scale_factor.unwrap_or(wm_scale_factor().round() as u32)
    };

    let (width, height) = state.window_size();
    let size = PhysicalSize::new(width * scale_factor, height * scale_factor);

    let mut input = WinitInputHelper::new();
    let window = setup_window(size, config.position, &event_loop);

    let mut pixels = {
        let window_size = window.inner_size();
        let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &window);
        PixelsBuilder::new(width, height, surface_texture)
            .clear_color({
                let [r, g, b, a] = config.background.map(|v| v as f64 / 255.0);
                pixels::wgpu::Color { r, g, b, a }
            })
            .blend_state(BlendState::REPLACE) // TODO: Investigate rendering weirdness.
            .build()?
    };

    event_loop.run(move |event, _, control_flow| {
        control_flow.set_wait_timeout(std::time::Duration::from_millis(500));

        match event {
            Event::NewEvents(winit::event::StartCause::ResumeTimeReached { .. }) => {
                window.request_redraw()
            }
            Event::RedrawRequested(_) => {
                // Clear the screen before drawing.
                pixels
                    .frame_mut()
                    .array_chunks_mut()
                    .for_each(|px| *px = state.background);

                // Update the state, then draw.
                state.update();
                state.draw(&mut pixels);

                // Try to render.
                if let Err(err) = pixels.render() {
                    eprintln!("ERROR: {err}");
                    *control_flow = ControlFlow::Exit;
                    return;
                }
            }
            _ => (),
        }

        if input.update(&event) {
            // Close events.
            if input.close_requested() {
                eprintln!("INFO:  Close requested. Bye :)");
                *control_flow = ControlFlow::Exit;
                return;
            }

            // Resize the window.
            if let Some(size) = input.window_resized() {
                eprintln!("INFO:  Ignoring resize request {size:?}");
            }
        }
    });
}
