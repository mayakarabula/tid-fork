#![feature(array_chunks, iter_array_chunks, slice_flatten, iter_intersperse)]

mod config;
mod font;
mod state;

use battery::Manager;
use config::{configure, Pixel, PIXEL_SIZE};
use font::Font;
use state::{Element, History, State};

use pixels::{Pixels, PixelsBuilder, SurfaceTexture};
use sysinfo::{System, SystemExt};
use winit::dpi::{LogicalPosition, PhysicalSize};
use winit::event::Event;
use winit::event_loop::{ControlFlow, EventLoop};
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
use winit::platform::x11::{WindowBuilderExtX11, XWindowType};
use winit::window::{Window, WindowBuilder, WindowLevel};
use winit_input_helper::WinitInputHelper;

const WINDOW_NAME: &str = env!("CARGO_BIN_NAME");

#[derive(Debug, Clone)]
struct Block {
    height: usize,
    pixels: Vec<Pixel>,
}

impl Block {
    fn width(&self) -> usize {
        self.pixels.len() / self.height
    }

    fn rows(&self) -> std::slice::ChunksExact<'_, Pixel> {
        self.pixels.chunks_exact(self.width())
    }

    fn draw_onto_pixels(self, pixels: &mut Pixels, start_x: usize) {
        let size = pixels.texture().size();
        for (y, row) in self.rows().enumerate() {
            let idx = (y * size.width as usize + start_x) * PIXEL_SIZE;
            let row_bytes = row.flatten();
            pixels.frame_mut()[idx..idx + row_bytes.len()].copy_from_slice(row_bytes);
        }
    }
}

trait Draw {
    fn draw(&self, state: &State) -> Block;
}

impl Draw for &str {
    fn draw(&self, state: &State) -> Block {
        let height = state.font.height();
        let glyphs = self.chars().flat_map(|ch| state.font.glyph(ch));
        let width: usize = glyphs.clone().map(|g| g.width()).sum();
        let mut pixels = vec![state.background; height * width];
        let mut x0 = 0;
        for g in glyphs {
            for (y, row) in g.rows().enumerate() {
                for (xg, &cell) in row.iter().enumerate() {
                    let x = x0 + xg;
                    let idx = y * width + x;
                    pixels[idx] = if cell {
                        state.foreground
                    } else {
                        state.background
                    };
                }
            }
            x0 += g.width();
        }

        Block { height, pixels }
    }
}

impl Draw for String {
    fn draw(&self, state: &State) -> Block {
        self.as_str().draw(state)
    }
}

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

    builder.build(event_loop).unwrap()
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

    let padding_left = 3;
    let elements = [
        Element::Padding(padding_left),
        Element::Date(Default::default()),
        Element::Space,
        Element::Time(Default::default()),
        Element::Space,
        Element::Label("bat".to_string()),
        Element::Battery(Default::default()),
        Element::Space,
        Element::Label("mem".to_string()),
        Element::Mem(Default::default()),
        Element::Space,
        Element::Label("cpu".to_string()),
        Element::Cpu(Default::default()),
        Element::Space,
        Element::CpuGraph(History::new(120)),
        Element::Space,
        Element::PlaybackState(Default::default()),
    ];
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
        elements.into(),
    );
    let (width, height) = state.window_size();
    let size = PhysicalSize::new(width, height);

    let event_loop = EventLoop::new();
    let mut input = WinitInputHelper::new();
    let window = setup_window(size, config.position, &event_loop);

    let mut pixels = {
        let window_size = window.inner_size();
        let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &window);
        PixelsBuilder::new(width, height, surface_texture)
            .clear_color(pixels::wgpu::Color::TRANSPARENT)
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
            if let Some(_size) = input.window_resized() {
                eprintln!("bro we don't even do resizes here");
            }
        }
    });
}
