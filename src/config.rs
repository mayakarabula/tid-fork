use std::fs::File;
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use lexopt::{Arg, Parser, ValueExt};

const CONFIG_FILE_PATH: &str = "/etc/tid/tid.config";

const DEFAULT_FONT_DIR: &str = "/etc/tid/fonts";
const DEFAULT_FONT: &str = "cream12.uf2";
const DEFAULT_MPD_ADDR: &str = "127.0.0.1:6600";

pub type Pixel = [u8; PIXEL_SIZE];
pub const PIXEL_SIZE: usize = 4;
const BACKGROUND: Pixel = [0x00; PIXEL_SIZE];
const FOREGROUND: Pixel = [0xff; PIXEL_SIZE];
const COLOR_PREFIX: &str = "0x";

pub struct Config {
    pub font_path: Box<Path>,
    pub foreground: Pixel,
    pub background: Pixel,
    pub mpd_addr: SocketAddr,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font_path: PathBuf::from_iter([DEFAULT_FONT_DIR, DEFAULT_FONT]).into_boxed_path(),
            foreground: FOREGROUND,
            background: BACKGROUND,
            mpd_addr: SocketAddr::from_str(DEFAULT_MPD_ADDR)
                .expect("DEFAULT_MPD_ADDR must be valid"),
        }
    }
}

// TODO: Rename this to be more appropriate for the stage before config but not necessarily args.
#[derive(Default)]
struct Args {
    pub font_path: Option<PathBuf>,
    pub foreground: Option<Pixel>,
    pub background: Option<Pixel>,
    pub mpd_addr: Option<SocketAddr>,
}

// TODO: Implement proper error type.
fn parse_config(config: &str) -> Result<Args, String> {
    let mut args = Args::default();

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
        let keyword = tokens.next().ok_or(String::from("expected keyword"))?;
        let argument = tokens
            .next()
            .ok_or(String::from("expected argument after keyword"))?;

        match keyword {
            "font_name" => args.font_path = Some(PathBuf::from_iter([DEFAULT_FONT_DIR, argument])),
            "font_path" => {
                args.font_path = Some(PathBuf::from_str(argument).map_err(|err| err.to_string())?)
            }
            "foreground" => {
                let stripped = argument.strip_prefix(COLOR_PREFIX).ok_or(format!(
                    "color values must be prefixed with '{COLOR_PREFIX}'"
                ))?;
                let num = u32::from_str_radix(stripped, 16).map_err(|e| e.to_string())?;
                args.foreground = Some(num.to_be_bytes());
            }
            "background" => {
                let stripped = argument.strip_prefix(COLOR_PREFIX).ok_or(format!(
                    "color values must be prefixed with '{COLOR_PREFIX}'"
                ))?;
                let num = u32::from_str_radix(stripped, 16).map_err(|e| e.to_string())?;
                args.background = Some(num.to_be_bytes());
            }
            "mpd_addr" => {
                args.mpd_addr = Some(SocketAddr::from_str(argument).map_err(|err| err.to_string())?)
            }
            unknown => return Err(format!("unknown keyword '{unknown}'")),
        }
    }

    Ok(args)
}

fn parse_args() -> Result<Args, lexopt::Error> {
    let mut args = Args::default();

    let mut parser = Parser::from_env();
    while let Some(arg) = parser.next()? {
        match arg {
            Arg::Short('n') | Arg::Long("font-name") => {
                args.font_path = Some(PathBuf::from_iter([
                    DEFAULT_FONT_DIR,
                    &parser.value()?.string()?,
                ]))
            }
            Arg::Short('p') | Arg::Long("font-path") => {
                args.font_path = Some(PathBuf::from(parser.value()?))
            }
            Arg::Long("fg") => {
                let hex = parser.value()?.string()?;
                let stripped = hex.trim().strip_prefix(COLOR_PREFIX).ok_or_else(|| {
                    format!("color values must be prefixed with '{COLOR_PREFIX}'")
                })?;
                let num = u32::from_str_radix(stripped, 16).map_err(|e| e.to_string())?;
                args.foreground = Some(num.to_be_bytes());
            }
            Arg::Long("bg") => {
                let hex = parser.value()?.string()?;
                let stripped = hex.trim().strip_prefix(COLOR_PREFIX).ok_or_else(|| {
                    format!("color values must be prefixed with '{COLOR_PREFIX}'")
                })?;
                let num = u32::from_str_radix(stripped, 16).map_err(|e| e.to_string())?;
                args.background = Some(num.to_be_bytes());
            }
            Arg::Long("mpd-address") => {
                args.mpd_addr = Some(
                    SocketAddr::from_str(&parser.value()?.string()?)
                        .map_err(|err| lexopt::Error::Custom(Box::new(err)))?,
                )
            }
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

    Ok(args)
}

/// Create a configuration based on defaults, followed by config files, and finally command line
/// arguments.
pub fn configure() -> Result<Config, Box<dyn std::error::Error>> {
    let config_file_path = PathBuf::from_str(CONFIG_FILE_PATH)?;
    let config_file_args = match File::open(&config_file_path) {
        Ok(mut config_file) => {
            let mut config_str = String::new();
            config_file.read_to_string(&mut config_str)?;
            Some(parse_config(&config_str)?)
        }
        Err(err) => {
            eprintln!("ERROR: problem reading '{config_file_path:?}': {err}");
            eprintln!("INFO:  '{config_file_path:?}' not found");
            None
        }
    };
    let command_line_args = Some(parse_args()?);

    let mut config = Config::default();
    for args in [config_file_args, command_line_args].into_iter().flatten() {
        // TODO: I don't like this pattern, tbh.
        if let Some(font_path) = args.font_path {
            config.font_path = font_path.into_boxed_path()
        }
        if let Some(foreground) = args.foreground {
            config.foreground = foreground
        }
        if let Some(background) = args.background {
            config.background = background
        }
        if let Some(mpd_addr) = args.mpd_addr {
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
    eprintln!("                      (default: {DEFAULT_MPD_ADDR})");
    eprintln!("    --version   -v    Display function.");
    eprintln!("    --help      -h    Display help.");
    eprintln!();
    eprintln!("{BIN} {VERSION} by {AUTHORS}, 2023.");
}
