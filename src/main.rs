#![feature(array_chunks, iter_array_chunks, slice_flatten, iter_intersperse)]

mod font;
mod state;

use std::path::PathBuf;
use std::str::FromStr;

use font::{Font, WrappedFont};
use state::{Element, State, History};

use pixels::{Pixels, PixelsBuilder, SurfaceTexture};
use sysinfo::{System, SystemExt};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::Event;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::platform::wayland::WindowBuilderExtWayland;
use winit::window::{WindowBuilder, WindowLevel};
use winit_input_helper::WinitInputHelper;

const DEFAULT_FONT_DIR: &str = "/etc/tid/fonts";
const DEFAULT_FONT: &str = "cream12.uf2";

const PIXEL_SIZE: usize = 4;
type Pixel = [u8; PIXEL_SIZE];
const BACKGROUND: Pixel = [0x00; PIXEL_SIZE];
const FOREGROUND: Pixel = [0xff; PIXEL_SIZE];

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
    fn draw(&self, font: &WrappedFont) -> Block;
}

impl Draw for String {
    fn draw(&self, font: &WrappedFont) -> Block {
        let height = font.height();
        let glyphs = self.chars().flat_map(|ch| font.glyph(ch));
        let width: usize = glyphs.clone().map(|g| g.width()).sum();
        let mut pixels = vec![BACKGROUND; height * width];
        let mut x0 = 0;
        for g in glyphs {
            for (y, row) in g.rows().enumerate() {
                for (xg, &cell) in row.iter().enumerate() {
                    let x = x0 + xg;
                    let idx = y * width + x;
                    pixels[idx] = if cell { FOREGROUND } else { BACKGROUND };
                }
            }
            x0 += g.width();
        }

        Block { height, pixels }
    }
}

fn main() -> Result<(), pixels::Error> {
    let mut args = std::env::args().skip(1);
    let flag = args.next();
    let value = args.next();
    let font_path = if let (Some(flag), Some(value)) = (flag, value) {
        match (flag.as_str(), value) {
            ("--font-name", font_name) => PathBuf::from_iter([DEFAULT_FONT_DIR, &font_name]),
            ("--font-path", font_path) => PathBuf::from_str(&font_path).unwrap(),
            _ => PathBuf::from_iter([DEFAULT_FONT_DIR, DEFAULT_FONT]),
        }
    } else {
        PathBuf::from_iter([DEFAULT_FONT_DIR, DEFAULT_FONT])
    };

    let font = font::load_font(font_path.as_path());

    let padding_left = 3;
    let elements = [
        Element::Padding(padding_left),
        Element::Date(Default::default()),
        Element::Space,
        Element::Time(Default::default()),
        Element::Space,
        Element::Label("mem".to_string()),
        Element::Mem(Default::default()),
        Element::Space,
        Element::Label("cpu".to_string()),
        Element::Cpu(Default::default()),
        Element::Space,
        Element::CpuGraph(History::new(120)),
    ];
    let mut state = State::new(font, System::new(), FOREGROUND, BACKGROUND, elements.into());
    let (width, height) = state.window_size();

    let event_loop = EventLoop::new();
    let mut input = WinitInputHelper::new();
    let window = WindowBuilder::new()
        .with_inner_size(LogicalSize::new(width, height))
        .with_transparent(true)
        .with_decorations(false)
        .with_title("tid")
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_resizable(false)
        .with_active(false)
        .with_position(LogicalPosition::new(0, 0))
        .with_name("systat", "$instance")
        .build(&event_loop)
        .unwrap();

    let mut pixels = {
        let window_size = window.inner_size();
        let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &window);
        PixelsBuilder::new(width, height, surface_texture)
            .blend_state(pixels::wgpu::BlendState::REPLACE)
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
                pixels.frame_mut().fill(0x00);

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
