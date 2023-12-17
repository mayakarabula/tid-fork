use std::collections::VecDeque;
use std::str::FromStr;

use battery::Battery;
use chrono::{Datelike, Timelike};
use pixels::Pixels;
use sysinfo::{CpuExt, System, SystemExt};

use crate::config::{Pixel, PIXEL_SIZE};
use crate::font::Font;

const BATTERY_FULL_PERCENTAGE: f32 = 98.0;

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
        for gl in glyphs {
            let glyph_width = gl.width();
            for (y, row) in gl.enumerate() {
                for (xg, cell) in row.enumerate() {
                    let x = x0 + xg;
                    pixels[y * width + x] = if cell {
                        state.foreground
                    } else {
                        state.background
                    };
                }
            }
            x0 += glyph_width;
        }

        Block { height, pixels }
    }
}

impl Draw for String {
    fn draw(&self, state: &State) -> Block {
        self.as_str().draw(state)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct History<T>(VecDeque<T>);

impl<T: Default + Clone> History<T> {
    pub(crate) fn new(len: usize) -> Self {
        Self(vec![Default::default(); len].into())
    }
}

impl<T: Default + Clone> Default for History<T> {
    fn default() -> Self {
        Self::new(120)
    }
}

impl<T> History<T> {
    fn push(&mut self, value: T) {
        let len = self.0.len();
        self.0.push_front(value);
        self.0.truncate(len);
    }

    fn iter(&self) -> std::collections::vec_deque::Iter<'_, T> {
        self.0.iter()
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}

type DateTime = chrono::DateTime<chrono::Local>;

fn playback_state_symbol(state: mpd::State) -> &'static str {
    match state {
        mpd::State::Stop => "#",
        mpd::State::Play => ">",
        mpd::State::Pause => "\"",
    }
}

#[derive(Debug, Clone)]
pub enum Element {
    Padding(usize),
    Space,
    Label(String),
    Date(DateTime),
    Time(DateTime),
    Mem(f32),
    Cpu(f32),
    Battery(f32),
    CpuGraph(History<f32>),
    PlaybackState(mpd::State),
}

#[derive(Debug)]
pub enum ElementParseError {
    BadInteger(std::num::ParseIntError),
    UnknownElementName(String),
    UnknownArgumentedElementName(String),
}

impl std::fmt::Display for ElementParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ElementParseError::BadInteger(e) => e.fmt(f),
            ElementParseError::UnknownElementName(weird) => {
                write!(f, "unknown element name '{weird}'")
            }
            ElementParseError::UnknownArgumentedElementName(weird) => {
                write!(f, "unknown argumented element name '{weird}'")
            }
        }
    }
}

impl std::error::Error for ElementParseError {}

impl From<std::num::ParseIntError> for ElementParseError {
    fn from(value: std::num::ParseIntError) -> Self {
        Self::BadInteger(value)
    }
}

impl FromStr for Element {
    type Err = ElementParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Some elements take a user-specifiable argument. We take care of these first.
        if s.contains('(') && s.ends_with(')') {
            let (name, argument) = s.split_once('(').unwrap();
            let argument = argument.trim_end_matches(')');
            let element = match name {
                "padding" => Self::Padding(argument.parse::<usize>()?),
                "label" => Self::Label(argument.to_string()),
                "cpugraph" => Self::CpuGraph(History::new(argument.parse::<usize>()?)),
                weird => Err(ElementParseError::UnknownArgumentedElementName(
                    weird.to_string(),
                ))?,
            };
            return Ok(element);
        }

        let element = match s {
            "space" => Self::Space,
            "date" => Self::Date(Default::default()),
            "time" => Self::Time(Default::default()),
            "battery" => Self::Battery(Default::default()),
            "mem" => Self::Mem(Default::default()),
            "cpu" => Self::Cpu(Default::default()),
            "playbackstate" => Self::PlaybackState(Default::default()),
            weird => Err(ElementParseError::UnknownElementName(weird.to_string()))?,
        };
        Ok(element)
    }
}

impl Element {
    fn width_with_font(&self, font: &Font) -> usize {
        match self {
            Element::Padding(width) => *width,
            Element::Space => font.determine_width("  "),
            Element::Label(s) => font.determine_width(s),
            Element::Date(_) => font.determine_width("0000-00-00"),
            Element::Time(_) => font.determine_width("00:00:00"),
            Element::Mem(_) => font.determine_width("000%"),
            Element::Cpu(_) => font.determine_width("000%"),
            Element::Battery(_) => font.determine_width("000%"),
            Element::CpuGraph(hist) => hist.len(),
            Element::PlaybackState(_) => [mpd::State::Stop, mpd::State::Play, mpd::State::Pause]
                .map(|state| font.determine_width(playback_state_symbol(state)))
                .into_iter()
                .max()
                .unwrap(),
        }
    }

    fn alignment(&self) -> Alignment {
        match self {
            Element::Padding(_)
            | Element::Space
            | Element::Label(_)
            | Element::Mem(_)
            | Element::Cpu(_)
            | Element::Battery(_)
            | Element::CpuGraph(_)
            | Self::PlaybackState(_) => Alignment::Right,
            Element::Date(_) | Element::Time(_) => Alignment::Left,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Alignment {
    Left,
    Right,
}

pub struct State {
    pub font: Font,
    sys: System,
    battery: Option<Battery>,
    music: Option<mpd::Client>,
    pub foreground: Pixel,
    pub background: Pixel,
    elements: Vec<Element>,
}

impl State {
    pub fn new(
        font: Font,
        sys: System,
        battery: Option<Battery>,
        music: Option<mpd::Client>,
        foreground: Pixel,
        background: Pixel,
        elements: Vec<Element>,
    ) -> Self {
        Self {
            font,
            sys,
            music,
            battery,
            foreground,
            background,
            elements,
        }
    }

    pub fn window_size(&self) -> (u32, u32) {
        let width: usize = self
            .elements
            .iter()
            .map(|e| e.width_with_font(&self.font))
            .sum();
        let height = self.font.height();
        (width as u32, height as u32)
    }

    pub fn update(&mut self) {
        // We refresh these once. This is good practice anyways, but refreshing multiple
        // times in quick succession may return NaN's on MacOS, apparently.
        // Thanks, Maya for noticing this!
        self.sys.refresh_cpu();
        self.sys.refresh_memory();

        for element in self.elements.iter_mut() {
            match element {
                Element::Date(dt) | Element::Time(dt) => *dt = chrono::Local::now(),
                Element::Mem(avl) => {
                    let used = self.sys.used_memory() as f32;
                    let available = self.sys.total_memory() as f32;
                    *avl = used / available * 100.0;
                }
                Element::Cpu(avg) => {
                    // FIXME: Sometimes on (at least) macOS, this returns NaN. This would crash the
                    // program, so we have a NaN check when drawing the element.
                    let cpus = self.sys.cpus();
                    *avg = cpus.iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / cpus.len() as f32;
                }
                Element::Battery(full) => {
                    if let Some(bat) = &mut self.battery {
                        let _ = bat.refresh();
                        *full = bat
                            .state_of_charge()
                            .get::<battery::units::ratio::percent>();
                        // If the battery is basically full, just set it to 100%.
                        if *full > BATTERY_FULL_PERCENTAGE {
                            *full = 100.0
                        }
                    }
                }
                Element::CpuGraph(hist) => {
                    let cpus = self.sys.cpus();
                    let avg =
                        cpus.iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / cpus.len() as f32;
                    hist.push(avg);
                }
                Element::PlaybackState(state) => {
                    // If we have access to mpd, and we get Some(Status) when we ask it, change the
                    // state to that status' state.
                    if let Some(status) = self.music.as_mut().and_then(|music| music.status().ok())
                    {
                        *state = status.state
                    }
                }
                Element::Label(_) | Element::Padding(_) | Element::Space => {}
            }
        }
    }

    pub fn draw(&self, pixels: &mut Pixels) {
        let mut x = 0;
        for element in &self.elements {
            let block = match element {
                Element::Padding(width) => {
                    x += width;
                    continue;
                }
                Element::Space => {
                    x += self.font.determine_width("  ");
                    continue;
                }
                Element::Label(s) => s.draw(self),
                Element::Date(dt) => {
                    format!("{:04}-{:02}-{:02}", dt.year(), dt.month(), dt.day()).draw(self)
                }
                Element::Time(dt) => {
                    format!("{:02}:{:02}:{:02}", dt.hour(), dt.minute(), dt.second()).draw(self)
                }
                Element::Mem(val) | Element::Cpu(val) | Element::Battery(val) => {
                    if val.is_nan() {
                        "---%".draw(self)
                    } else {
                        format!("{val:>3.0}%").draw(self)
                    }
                }
                Element::CpuGraph(hist) => {
                    let height = self.window_size().1 as usize;
                    let width = hist.len();
                    let mut pixels = vec![self.background; height * width];

                    for (x, usage) in hist.iter().enumerate() {
                        let blank = height - ((usage / 100.0) * height as f32) as usize;
                        for y in 0..height {
                            let px = if y < blank {
                                self.background
                            } else {
                                self.foreground
                            };
                            let idx = y * width + x;
                            pixels[idx] = px;
                        }
                    }

                    Block { height, pixels }
                }
                Element::PlaybackState(state) => playback_state_symbol(*state).draw(self),
            };

            // We want to align some elements like cpu and memory percentages to the right, since
            // their least significant digits change frequently and often displays a '1'.
            let block_width = block.width();
            let overshoot = element.width_with_font(&self.font) - block_width;

            match element.alignment() {
                Alignment::Left => {
                    block.draw_onto_pixels(pixels, x);
                    x += overshoot;
                }
                Alignment::Right => {
                    x += overshoot;
                    block.draw_onto_pixels(pixels, x);
                }
            }

            x += block_width;
        }
    }
}
