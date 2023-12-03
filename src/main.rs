#![feature(array_chunks, iter_array_chunks, slice_flatten, iter_intersperse)]

mod font;
mod state;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use battery::Manager;
use font::Font;
use state::{Element, History, State};

use lexopt::{Arg, Parser, ValueExt};
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

const MPD_ADDR: &str = "127.0.0.1:6600";

const PIXEL_SIZE: usize = 4;
type Pixel = [u8; PIXEL_SIZE];
const BACKGROUND: Pixel = [0x00; PIXEL_SIZE];
const FOREGROUND: Pixel = [0xff; PIXEL_SIZE];
const COLOR_PREFIX: &str = "0x";

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

impl Draw for String {
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

struct Args {
    font_path: Box<Path>,
    foreground: Pixel,
    background: Pixel,
    mpd_addr: SocketAddr,
}

fn usage(bin: &str) {
    const DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");
    const BIN: &str = env!("CARGO_BIN_NAME");
    const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    const DEFAULT_FG: u32 = u32::from_be_bytes(FOREGROUND);
    const DEFAULT_BG: u32 = u32::from_be_bytes(BACKGROUND);
    eprintln!("{DESCRIPTION}");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("    {bin} [OPTIONS]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("    --font-name -n    Set the font name from the default directory.");
    eprintln!("                      (default: '{DEFAULT_FONT}' in '{DEFAULT_FONT_DIR}')");
    eprintln!("    --font-path -p    Set the font path.");
    eprintln!("    --fg              Specify the foreground color as an rgba hex string.");
    eprintln!("                      (default: {COLOR_PREFIX}{DEFAULT_FG:08x})");
    eprintln!("    --bg              Specify the background color as an rgba hex string.");
    eprintln!("                      (default: {COLOR_PREFIX}{DEFAULT_BG:08x})");
    eprintln!("    --mpd-address     Specify the address for the mpd connection.");
    eprintln!("                      (default: {MPD_ADDR})");
    eprintln!("    --version   -v    Display function.");
    eprintln!("    --help      -h    Display help.");
    eprintln!();
    eprintln!("{BIN} {VERSION} by {AUTHORS}, 2023.");
}

fn parse_args() -> Result<Args, lexopt::Error> {
    let mut font_path = PathBuf::from_iter([DEFAULT_FONT_DIR, DEFAULT_FONT]);
    let mut foreground = FOREGROUND;
    let mut background = BACKGROUND;
    let mut mpd_addr = MPD_ADDR.to_string();

    let mut parser = Parser::from_env();
    while let Some(arg) = parser.next()? {
        match arg {
            Arg::Short('n') | Arg::Long("font-name") => {
                font_path = PathBuf::from_iter([DEFAULT_FONT_DIR, &parser.value()?.string()?]);
            }
            Arg::Short('p') | Arg::Long("font-path") => {
                font_path = PathBuf::from(parser.value()?);
            }
            Arg::Long("fg") => {
                let hex = parser.value()?.string()?;
                let stripped = hex.trim().strip_prefix(COLOR_PREFIX).ok_or_else(|| {
                    format!("color values must be prefixed with '{COLOR_PREFIX}'")
                })?;
                let num = u32::from_str_radix(stripped, 16).map_err(|e| e.to_string())?;
                foreground = num.to_be_bytes();
            }
            Arg::Long("bg") => {
                let hex = parser.value()?.string()?;
                let stripped = hex.trim().strip_prefix(COLOR_PREFIX).ok_or_else(|| {
                    format!("color values must be prefixed with '{COLOR_PREFIX}'")
                })?;
                let num = u32::from_str_radix(stripped, 16).map_err(|e| e.to_string())?;
                background = num.to_be_bytes();
            }
            Arg::Long("mpd-address") => mpd_addr = parser.value()?.string()?,
            Arg::Short('v') | Arg::Long("version") => {
                println!("{}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            Arg::Short('h') | Arg::Long("help") => {
                usage(parser.bin_name().unwrap_or(env!("CARGO_BIN_NAME")));
                std::process::exit(0);
            }
            _ => return Err(arg.unexpected()),
        }
    }

    Ok(Args {
        font_path: font_path.into_boxed_path(),
        foreground,
        background,
        mpd_addr: SocketAddr::from_str(&mpd_addr)
            .map_err(|err| lexopt::Error::Custom(Box::new(err)))?,
    })
}

fn main() -> Result<(), pixels::Error> {
    let args = match parse_args() {
        Ok(args) => args,
        Err(err) => {
            eprintln!("ERROR: {err}");
            eprintln!("Run with --help for usage information.");
            std::process::exit(1);
        }
    };

    let font = font::load_font(&args.font_path);

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
            Ok(mut bats) => bats.next().map(|err| err.ok()).flatten(),
            Err(_) => None,
        }),
        mpd::Client::connect(args.mpd_addr).ok(),
        args.foreground,
        args.background,
        elements.into(),
    );
    let (width, height) = state.window_size();
    let size = LogicalSize::new(width, height);

    let event_loop = EventLoop::new();
    let mut input = WinitInputHelper::new();
    let window = WindowBuilder::new()
        .with_inner_size(size)
        .with_min_inner_size(size)
        .with_max_inner_size(size)
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
