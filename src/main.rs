#![feature(array_chunks, iter_array_chunks, slice_flatten)]

mod font;

use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;

use font::Font;

use chrono::Timelike;
use pixels::{Pixels, PixelsBuilder, SurfaceTexture};
use sysinfo::{CpuExt, System, SystemExt};
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
    fn rows(&self) -> std::slice::ChunksExact<'_, Pixel> {
        let width = self.pixels.len() / self.height;
        self.pixels.chunks_exact(width)
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
    fn draw(&self, font: &Font) -> Block;
}

impl Draw for String {
    fn draw(&self, font: &Font) -> Block {
        let height = font.height();
        let glyphs = self.chars().flat_map(|ch| font.glyph(ch));
        let width: usize = glyphs.clone().map(|g| g.width).sum();
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
            x0 += g.width;
        }

        Block { height, pixels }
    }
}

fn report_time(t: std::time::Instant, msg: &str) {
    let ms = t.elapsed().as_secs_f32() * 1000.0;
    eprintln!("{ms:>9.5} ms: {msg}");
}

fn load_font<P: AsRef<Path>>(path: P) -> Font {
    let mut file = std::fs::File::open(path).unwrap();
    let mut buf = [0; font::FILE_SIZE];
    file.read_exact(&mut buf).unwrap();
    let font = Font::from_uf2(&buf);
    font
}

fn main() -> Result<(), pixels::Error> {
    let cpu_graph_width = 120;
    let mut sys = System::new();
    let mut cpu_hist = VecDeque::with_capacity(cpu_graph_width);

    let font_path = std::path::PathBuf::from_iter([DEFAULT_FONT_DIR, DEFAULT_FONT]);
    let font = { load_font(font_path) };

    let padding_left = 3;
    let space = font.determine_width("  ");
    let clock_width = font.determine_width("00:00:00");
    let cpu_width = font.determine_width("c100%");
    let mem_width = font.determine_width("m100%");

    let set_size_by_font = |font: &Font| {
        let width = padding_left
            + clock_width
            + space
            + mem_width
            + space
            + cpu_width
            + space
            + cpu_graph_width;
        let height = font.height();
        (width as u32, height as u32)
    };
    let (width, height) = set_size_by_font(&font);

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

                // Get the info.
                let time = chrono::Local::now();
                let clock = format!(
                    "{:02}:{:02}:{:02}",
                    time.hour(),
                    time.minute(),
                    time.second()
                );
                let cpu_avg = {
                    sys.refresh_cpu();
                    let cpus = sys.cpus();
                    let avg =
                        cpus.iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / cpus.len() as f32;
                    cpu_hist.push_front(avg);
                    cpu_hist.truncate(cpu_graph_width);
                    format!("c{avg:>3.0}%")
                };
                let mem = {
                    sys.refresh_memory();
                    let used = sys.used_memory();
                    let available = sys.available_memory();
                    let perc = (used as f32 / available as f32) * 100.0;
                    format!("m{perc:>3.0}%")
                };

                // Draw the info.
                let mut x = padding_left;
                clock.draw(&font).draw_onto_pixels(&mut pixels, x);
                x += clock_width + space;
                mem.draw(&font).draw_onto_pixels(&mut pixels, x);
                x += mem_width + space;
                cpu_avg.draw(&font).draw_onto_pixels(&mut pixels, x);
                // x += cpu_width + space;

                // Draw the cpu graph.
                let mut x0 = width as usize - cpu_graph_width;
                for usage in cpu_hist.iter() {
                    let blank = height as usize - ((usage / 100.0) * height as f32) as usize;
                    for y in 0..height as usize {
                        let px = if y < blank { BACKGROUND } else { FOREGROUND };
                        let idx = (y * width as usize + x0) * PIXEL_SIZE;
                        pixels.frame_mut()[idx..idx + PIXEL_SIZE].copy_from_slice(&px);
                    }
                    x0 += 1
                }

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
