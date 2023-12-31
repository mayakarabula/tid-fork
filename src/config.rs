use std::fs::File;
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use lexopt::{Arg, Parser, ValueExt};
use winit::dpi::LogicalPosition;

use crate::state::Element;

const CONFIG_FILE_PATH: &str = "/etc/tid/tid.config";

const DEFAULT_FONT_DIR: &str = "/etc/tid/fonts";
const DEFAULT_FONT: &str = "cream12.uf2";
const DEFAULT_MPD_ADDR: &str = "127.0.0.1:6600";
const DEFAULT_BACKGROUND: Pixel = [0x00; PIXEL_SIZE];
const DEFAULT_FOREGROUND: Pixel = [0xff; PIXEL_SIZE];

pub type Pixel = [u8; PIXEL_SIZE];
pub const PIXEL_SIZE: usize = 4;
const COLOR_PREFIX: &str = "0x";

pub struct Config {
    pub elements: Vec<Element>,
    pub font_path: Box<Path>,
    pub foreground: Pixel,
    pub background: Pixel,
    pub position: LogicalPosition<u32>,
    pub mpd_addr: SocketAddr,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            elements: vec![
                Element::Padding(3),
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
                Element::CpuGraph(Default::default()),
                Element::Space,
                Element::PlaybackState(Default::default()),
                Element::Padding(3),
            ],
            font_path: PathBuf::from_iter([DEFAULT_FONT_DIR, DEFAULT_FONT]).into_boxed_path(),
            foreground: DEFAULT_FOREGROUND,
            background: DEFAULT_BACKGROUND,
            position: LogicalPosition::default(),
            mpd_addr: SocketAddr::from_str(DEFAULT_MPD_ADDR)
                .expect("DEFAULT_MPD_ADDR must be valid"),
        }
    }
}

#[derive(Default)]
struct ConfigBuilder {
    pub elements: Option<Vec<String>>,
    pub font_path: Option<PathBuf>,
    pub foreground: Option<Pixel>,
    pub background: Option<Pixel>,
    pub position: Option<(u32, u32)>,
    pub mpd_addr: Option<SocketAddr>,
}

impl ConfigBuilder {
    fn set_elements(&mut self, elements: impl Iterator<Item = String>) {
        self.elements = Some(elements.collect());
    }

    fn set_font_path(&mut self, font_path: PathBuf) {
        self.font_path = Some(font_path);
    }

    fn set_foreground(&mut self, foreground: Pixel) {
        self.foreground = Some(foreground);
    }

    fn set_background(&mut self, background: Pixel) {
        self.background = Some(background);
    }

    fn set_position(&mut self, position: (u32, u32)) {
        self.position = Some(position);
    }

    fn set_mpd_addr(&mut self, mpd_addr: SocketAddr) {
        self.mpd_addr = Some(mpd_addr);
    }
}

fn parse_color(hex: &str) -> Result<u32, String> {
    let stripped = hex.strip_prefix(COLOR_PREFIX).ok_or(format!(
        "color values must be prefixed with '{COLOR_PREFIX}'"
    ))?;
    u32::from_str_radix(stripped, 16).map_err(|e| e.to_string())
}

fn parse_position(pos: &str) -> Result<(u32, u32), String> {
    let (x, y) = pos.split_once(',').ok_or(
        "position must be formatted as 'x,y' (no space!) where x and y are unsigned integers",
    )?;
    let x: u32 = x
        .parse()
        .map_err(|err| format!("error while parsing x value in position: {err}"))?;
    let y: u32 = y
        .parse()
        .map_err(|err| format!("error while parsing y value in position: {err}"))?;
    Ok((x, y))
}

fn parse_config(config: &str) -> Result<ConfigBuilder, String> {
    let mut cfg = ConfigBuilder::default();

    // Go through each line, stripping of comments, trimming each line, and skipping empty lines.
    for line in config
        .lines()
        .map(|ln| {
            if let Some((before, _comment)) = ln.split_once('#') {
                before
            } else {
                ln
            }
            .trim()
        })
        .filter(|ln| !ln.is_empty())
    {
        let mut tokens = line.split_whitespace();
        // This unwrap is safe since we filter out lines that are empty after trimming whitespace.
        // Lines without at least a keyword can never reach this point.
        let keyword = tokens.next().unwrap();
        let arguments: Vec<_> = tokens.collect();
        let first_argument = arguments
            .first()
            .ok_or(String::from("expected argument after keyword"))?;

        match keyword {
            "elements" => cfg.set_elements(arguments.iter().map(|s| s.to_string())),
            "font_name" => {
                cfg.set_font_path(PathBuf::from_iter([DEFAULT_FONT_DIR, first_argument]))
            }
            "font_path" => cfg.set_font_path(PathBuf::from(first_argument)),
            "foreground" => cfg.set_foreground(parse_color(first_argument)?.to_be_bytes()),
            "background" => cfg.set_background(parse_color(first_argument)?.to_be_bytes()),
            "position" => cfg.set_position(parse_position(first_argument)?),
            "mpd_addr" => cfg
                .set_mpd_addr(SocketAddr::from_str(first_argument).map_err(|err| err.to_string())?),

            unknown => return Err(format!("unknown keyword '{unknown}'")),
        }
    }

    Ok(cfg)
}

fn parse_args() -> Result<ConfigBuilder, lexopt::Error> {
    let mut cfg = ConfigBuilder::default();

    let mut parser = Parser::from_env();
    while let Some(arg) = parser.next()? {
        match arg {
            Arg::Long("elements") => {
                let elems = parser.value()?.string()?;
                cfg.set_elements(elems.split_whitespace().map(|s| s.to_string()))
            }
            Arg::Short('n') | Arg::Long("font-name") => cfg.set_font_path(PathBuf::from_iter([
                DEFAULT_FONT_DIR,
                &parser.value()?.string()?,
            ])),
            Arg::Short('p') | Arg::Long("font-path") => {
                cfg.set_font_path(PathBuf::from(parser.value()?))
            }
            Arg::Long("fg") => {
                let hex = parser.value()?.string()?;
                cfg.set_foreground(parse_color(&hex)?.to_be_bytes());
            }
            Arg::Long("bg") => {
                let hex = parser.value()?.string()?;
                cfg.set_background(parse_color(&hex)?.to_be_bytes());
            }
            Arg::Long("position") => {
                let argument = parser.value()?.string()?;
                cfg.set_position(parse_position(&argument)?);
            }
            Arg::Long("mpd-address") => cfg.set_mpd_addr(
                SocketAddr::from_str(&parser.value()?.string()?)
                    .map_err(|err| lexopt::Error::Custom(Box::new(err)))?,
            ),
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

    Ok(cfg)
}

/// Create a configuration based on defaults, followed by config files, and finally command line
/// arguments.
pub fn configure() -> Result<Config, Box<dyn std::error::Error>> {
    let config_file_path = PathBuf::from_str(CONFIG_FILE_PATH)?;
    let config_file_cfg = match File::open(&config_file_path) {
        Ok(mut config_file) => {
            let mut config_str = String::new();
            config_file.read_to_string(&mut config_str)?;
            Some(parse_config(&config_str).map_err(|err| {
                format!("problem parsing config file {config_file_path:?}: {err}")
            })?)
        }
        Err(err) => {
            eprintln!("ERROR: problem reading {config_file_path:?}: {err}");
            None
        }
    };
    let command_line_cfg =
        Some(parse_args().map_err(|err| format!("problem reading command line arguments: {err}"))?);

    let mut config = Config::default();
    for cfg in [config_file_cfg, command_line_cfg].into_iter().flatten() {
        // TODO: I don't like this pattern, tbh.
        if let Some(elements) = cfg.elements {
            config.elements = elements
                .iter()
                .map(|elem| Element::from_str(elem))
                .collect::<Result<_, _>>()
                .map_err(|err| format!("problem encountered while parsing elements: {err}"))?
        }
        if let Some(font_path) = cfg.font_path {
            config.font_path = font_path.into_boxed_path()
        }
        if let Some(foreground) = cfg.foreground {
            config.foreground = foreground
        }
        if let Some(background) = cfg.background {
            config.background = background
        }
        if let Some(position) = cfg.position {
            config.position = LogicalPosition::from(position)
        }
        if let Some(mpd_addr) = cfg.mpd_addr {
            config.mpd_addr = mpd_addr
        }
    }

    Ok(config)
}

fn usage(bin: &str) {
    const DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");
    const BIN: &str = env!("CARGO_BIN_NAME");
    const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    const DEFAULT_FG: u32 = u32::from_be_bytes(DEFAULT_FOREGROUND);
    const DEFAULT_BG: u32 = u32::from_be_bytes(DEFAULT_BACKGROUND);
    eprintln!("{DESCRIPTION}");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("    {bin} [OPTIONS]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("    --elements        Define the elements to be displayed.");
    eprintln!("                      This is a space-delimited list of any of the following");
    eprintln!("                      items:");
    eprintln!("                        - padding(<width>)       - space");
    eprintln!("                        - date                   - time");
    eprintln!("                        - label(<text>)          - battery");
    eprintln!("                        - mem                    - cpu");
    eprintln!("                        - cpugraph(<width>)      - playbackstate");
    eprintln!("    --font-name -n    Set the font name from the default directory.");
    eprintln!("                      (default: '{DEFAULT_FONT}' in '{DEFAULT_FONT_DIR}')");
    eprintln!("    --font-path -p    Set the font path.");
    eprintln!("    --fg              Specify the foreground color as an rgba hex string.");
    eprintln!("                      (default: {COLOR_PREFIX}{DEFAULT_FG:08x})");
    eprintln!("    --bg              Specify the background color as an rgba hex string.");
    eprintln!("                      (default: {COLOR_PREFIX}{DEFAULT_BG:08x})");
    eprintln!("    --position        Set the requested position to spawn the window.");
    eprintln!("                      Must be set as 'x,y' without a space, where x and y are");
    eprintln!("                      unsigned integers.  (default: '0,0')");
    eprintln!("    --mpd-address     Specify the address for the mpd connection.");
    eprintln!("                      (default: {DEFAULT_MPD_ADDR})");
    eprintln!("    --version   -v    Display function.");
    eprintln!("    --help      -h    Display help.");
    eprintln!();
    eprintln!("{BIN} {VERSION} by {AUTHORS}, 2023.");
}
