use std::collections::VecDeque;

use battery::Battery;
use chrono::{Datelike, Timelike};
use pixels::Pixels;
use sysinfo::{CpuExt, System, SystemExt};

use crate::font::{Font, WrappedFont};
use crate::{Block, Draw, Pixel};

#[derive(Debug, Clone)]
pub(crate) struct History<T>(VecDeque<T>);

impl<T: Default + Clone> History<T> {
    pub(crate) fn new(len: usize) -> Self {
        Self(vec![Default::default(); len].into())
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

impl Element {
    fn width_with_font(&self, font: &WrappedFont) -> usize {
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
    pub font: WrappedFont,
    sys: System,
    battery: Option<Battery>,
    music: Option<mpd::Client>,
    pub foreground: Pixel,
    pub background: Pixel,
    elements: Vec<Element>,
}

impl State {
    // TODO: I think a builder pattern would be nicer here, especially since that makes for nice
    // defaults.
    pub fn new(
        font: WrappedFont,
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
        for element in self.elements.iter_mut() {
            match element {
                Element::Date(dt) | Element::Time(dt) => *dt = chrono::Local::now(),
                Element::Mem(avl) => {
                    self.sys.refresh_memory();
                    let used = self.sys.used_memory();
                    let available = self.sys.available_memory();
                    *avl = used as f32 / available as f32 * 100.0;
                }
                Element::Cpu(avg) => {
                    self.sys.refresh_cpu();
                    let cpus = self.sys.cpus();
                    *avg = cpus.iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / cpus.len() as f32;
                }
                Element::Battery(full) => {
                    if let Some(bat) = &mut self.battery {
                        let _ = bat.refresh();
                        *full = bat
                            .state_of_charge()
                            .get::<battery::units::ratio::percent>();
                    }
                }
                Element::CpuGraph(hist) => {
                    self.sys.refresh_cpu();
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
                    format!("{val:>3.0}%").draw(self)
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
                Element::PlaybackState(state) => {
                    // FIXME: I think draw should just be able to take a &str, not a string per se?
                    playback_state_symbol(*state).draw(self)
                }
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
