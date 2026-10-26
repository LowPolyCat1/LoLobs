use clap::{Parser, ValueEnum};
use serde::Deserialize;
use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::str::FromStr;

const CONFIG_FILE_NAME: &str = "config.toml";
const DEFAULT_IP: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const DEFAULT_PORT: u16 = 2009;

/// The ranked queue that the overlay shows.
#[derive(Deserialize, ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Queue {
    #[default]
    Solo,
    Flex,
}

/// The maximum number of games in the `matches` message.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(try_from = "u64")]
pub struct MatchLimit(u8);

impl MatchLimit {
    pub const DEFAULT: Self = Self(5);
    const MIN: u64 = 1;
    const MAX: u64 = 20;

    pub fn get(self) -> usize {
        usize::from(self.0)
    }
}

impl Default for MatchLimit {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl TryFrom<u64> for MatchLimit {
    type Error = String;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        let in_range = u8::try_from(value)
            .ok()
            .filter(|_| (Self::MIN..=Self::MAX).contains(&value));
        in_range.map(Self).ok_or_else(|| {
            format!(
                "The match limit {} is not in the range {} to {}.",
                value,
                Self::MIN,
                Self::MAX
            )
        })
    }
}

impl FromStr for MatchLimit {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let value = text
            .parse::<u64>()
            .map_err(|_| format!("The match limit {} is not a number.", text))?;
        Self::try_from(value)
    }
}

/// How the server uses the saved session stats in `session.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionMode {
    /// The server does not read or write the file.
    Off,
    /// The server continues the saved session of the same account on the same day.
    Resume,
    /// The server ignores the saved session at startup and replaces it.
    Reset,
}

impl SessionMode {
    fn new(persist: bool, reset: bool) -> Self {
        match (persist, reset) {
            (false, _) => Self::Off,
            (true, false) => Self::Resume,
            (true, true) => Self::Reset,
        }
    }
}

/// Settings from the config file or from the command line. A missing value uses the value from the next source.
#[derive(Parser, Deserialize, Debug, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[command(
    version,
    about = "Starts the LoLobs overlay server. A command line value replaces the value from config.toml."
)]
pub struct Overrides {
    /// The IP address of the overlay server [default: 127.0.0.1]
    #[arg(long)]
    ip: Option<IpAddr>,
    /// The port of the overlay server [default: 2009]
    #[arg(long)]
    port: Option<u16>,
    /// The ranked queue that the overlay shows [default: solo]
    #[arg(long, value_enum)]
    queue: Option<Queue>,
    /// The maximum number of games in the match history, from 1 to 20 [default: 5]
    #[arg(long)]
    match_limit: Option<MatchLimit>,
    /// Do not save the session stats to session.json and do not read them at startup
    #[arg(
        long = "no-persist-session",
        num_args = 0,
        default_missing_value = "false"
    )]
    persist_session: Option<bool>,
    /// Ignore the saved session stats at startup and start a new session
    #[arg(long)]
    #[serde(skip)]
    reset_session: bool,
}

impl Overrides {
    fn or(self, fallback: Overrides) -> Overrides {
        Overrides {
            ip: self.ip.or(fallback.ip),
            port: self.port.or(fallback.port),
            queue: self.queue.or(fallback.queue),
            match_limit: self.match_limit.or(fallback.match_limit),
            persist_session: self.persist_session.or(fallback.persist_session),
            reset_session: self.reset_session || fallback.reset_session,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub ip: IpAddr,
    pub port: u16,
    pub queue: Queue,
    pub match_limit: MatchLimit,
    pub session: SessionMode,
}

impl From<Overrides> for Config {
    fn from(values: Overrides) -> Self {
        Self {
            ip: values.ip.unwrap_or(DEFAULT_IP),
            port: values.port.unwrap_or(DEFAULT_PORT),
            queue: values.queue.unwrap_or_default(),
            match_limit: values.match_limit.unwrap_or_default(),
            session: SessionMode::new(values.persist_session.unwrap_or(true), values.reset_session),
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(
                    f,
                    "Cannot read the config file {}: {}",
                    path.display(),
                    source
                )
            }
            Self::Parse { path, source } => {
                write!(
                    f,
                    "The config file {} is not valid: {}",
                    path.display(),
                    source
                )
            }
        }
    }
}

/// Returns the path of the file `name` in the folder of the executable.
pub fn next_to_exe(exe_path: Option<&Path>, name: &str) -> Option<PathBuf> {
    exe_path.and_then(Path::parent).map(|dir| dir.join(name))
}

/// Returns `None` when the file does not exist.
fn read_file(path: &Path) -> Result<Option<String>, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ConfigError::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn parse_file(path: &Path, text: &str) -> Result<Overrides, ConfigError> {
    toml::from_str(text).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn load_file(path: Option<&Path>) -> Result<Overrides, ConfigError> {
    let Some(path) = path else {
        return Ok(Overrides::default());
    };
    let Some(text) = read_file(path)? else {
        return Ok(Overrides::default());
    };
    parse_file(path, &text)
}

/// Reads the config file next to `exe_path` and applies the command line values on top of it.
pub fn load(exe_path: Option<&Path>, cli: Overrides) -> Result<Config, ConfigError> {
    let file = load_file(next_to_exe(exe_path, CONFIG_FILE_NAME).as_deref())?;
    Ok(Config::from(cli.or(file)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn unique_temp_dir() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("lolobs-config-{}-{}", std::process::id(), id));
        // A process ID can repeat between runs (always under wine), so a folder from an earlier run can exist.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn exe_with_config(text: &str) -> PathBuf {
        let dir = unique_temp_dir();
        std::fs::write(dir.join(CONFIG_FILE_NAME), text).expect("write config file");
        dir.join("LoLobs.exe")
    }

    fn parse(text: &str) -> Result<Overrides, ConfigError> {
        parse_file(Path::new(CONFIG_FILE_NAME), text)
    }

    fn cli(args: &[&str]) -> Overrides {
        Overrides::try_parse_from(std::iter::once("LoLobs").chain(args.iter().copied()))
            .expect("arguments parse")
    }

    fn limit(value: u64) -> MatchLimit {
        MatchLimit::try_from(value).expect("limit is valid")
    }

    #[test]
    fn full_file_sets_all_values() {
        let exe = exe_with_config(
            "ip = \"0.0.0.0\"\nport = 3000\nqueue = \"flex\"\nmatch_limit = 10\npersist_session = false\n",
        );

        let config = load(Some(&exe), Overrides::default()).expect("config loads");

        assert_eq!(
            config,
            Config {
                ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                port: 3000,
                queue: Queue::Flex,
                match_limit: limit(10),
                session: SessionMode::Off,
            }
        );
    }

    #[test]
    fn partial_file_uses_defaults_for_missing_keys() {
        let exe = exe_with_config("port = 3000\n");

        let config = load(Some(&exe), Overrides::default()).expect("config loads");

        assert_eq!(
            config,
            Config {
                port: 3000,
                ..Config::from(Overrides::default())
            }
        );
    }

    #[test]
    fn missing_file_uses_defaults() {
        let exe = unique_temp_dir().join("LoLobs.exe");

        let config = load(Some(&exe), Overrides::default()).expect("config loads");

        assert_eq!(
            config,
            Config {
                ip: DEFAULT_IP,
                port: 2009,
                queue: Queue::Solo,
                match_limit: limit(5),
                session: SessionMode::Resume,
            }
        );
    }

    #[test]
    fn missing_exe_path_uses_defaults() {
        let config = load(None, Overrides::default()).expect("config loads");

        assert_eq!(config, Config::from(Overrides::default()));
    }

    #[test]
    fn command_line_overrides_file() {
        let exe = exe_with_config("port = 3000\nqueue = \"flex\"\nmatch_limit = 10\n");
        let args = cli(&["--port", "4000", "--match-limit", "3"]);

        let config = load(Some(&exe), args).expect("config loads");

        assert_eq!(config.port, 4000);
        assert_eq!(config.queue, Queue::Flex);
        assert_eq!(config.match_limit, limit(3));
    }

    #[test]
    fn command_line_parses_all_flags() {
        let args = cli(&[
            "--ip",
            "192.168.0.5",
            "--port",
            "4000",
            "--queue",
            "flex",
            "--match-limit",
            "20",
            "--reset-session",
        ]);

        assert_eq!(
            Config::from(args),
            Config {
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 0, 5)),
                port: 4000,
                queue: Queue::Flex,
                match_limit: limit(20),
                session: SessionMode::Reset,
            }
        );
    }

    #[test]
    fn invalid_queue_in_file_names_the_value() {
        let err = parse("queue = \"duo\"\n").expect_err("queue is invalid");

        let message = err.to_string();
        assert!(message.contains(CONFIG_FILE_NAME), "{message}");
        assert!(message.contains("duo"), "{message}");
    }

    #[test]
    fn invalid_queue_on_command_line_is_an_error() {
        let result = Overrides::try_parse_from(["LoLobs", "--queue", "duo"]);

        let message = result.expect_err("queue is invalid").to_string();
        assert!(message.contains("duo"), "{message}");
    }

    #[test]
    fn invalid_limit_in_file_names_key_and_value() {
        let err = parse("match_limit = 25\n").expect_err("limit is invalid");

        let message = err.to_string();
        assert!(message.contains("match_limit"), "{message}");
        assert!(
            message.contains("The match limit 25 is not in the range 1 to 20."),
            "{message}"
        );
    }

    #[test]
    fn invalid_limit_on_command_line_names_the_value() {
        let result = Overrides::try_parse_from(["LoLobs", "--match-limit", "0"]);

        let message = result.expect_err("limit is invalid").to_string();
        assert!(
            message.contains("The match limit 0 is not in the range 1 to 20."),
            "{message}"
        );
    }

    #[test]
    fn limit_accepts_range_bounds() {
        assert_eq!(MatchLimit::try_from(1).map(MatchLimit::get), Ok(1));
        assert_eq!(MatchLimit::try_from(20).map(MatchLimit::get), Ok(20));
        assert!(MatchLimit::try_from(21).is_err());
        assert!(MatchLimit::try_from(300).is_err());
    }

    #[test]
    fn limit_text_that_is_not_a_number_is_an_error() {
        assert_eq!(
            "five".parse::<MatchLimit>(),
            Err("The match limit five is not a number.".to_string())
        );
    }

    #[test]
    fn example_file_contains_the_defaults() {
        let example = parse(include_str!("../config.example.toml")).expect("example parses");

        assert_eq!(Config::from(example), Config::from(Overrides::default()));
    }

    #[test]
    fn no_persist_session_flag_overrides_file() {
        let exe = exe_with_config("persist_session = true\n");

        let config = load(Some(&exe), cli(&["--no-persist-session"])).expect("config loads");

        assert_eq!(config.session, SessionMode::Off);
    }

    #[test]
    fn no_persist_session_wins_over_reset_session() {
        let args = cli(&["--no-persist-session", "--reset-session"]);

        assert_eq!(Config::from(args).session, SessionMode::Off);
    }

    #[test]
    fn reset_session_keeps_persistence_from_file() {
        let exe = exe_with_config("persist_session = true\n");

        let config = load(Some(&exe), cli(&["--reset-session"])).expect("config loads");

        assert_eq!(config.session, SessionMode::Reset);
    }

    #[test]
    fn no_persist_session_flag_takes_no_value() {
        let result = Overrides::try_parse_from(["LoLobs", "--no-persist-session=true"]);

        assert!(result.is_err());
    }

    #[test]
    fn reset_session_in_file_is_an_error() {
        let err = parse("reset_session = true\n").expect_err("key is unknown");

        assert!(err.to_string().contains("reset_session"), "{err}");
    }

    #[test]
    fn unknown_key_in_file_is_an_error() {
        let err = parse("session = 1\n").expect_err("key is unknown");

        assert!(err.to_string().contains("session"), "{err}");
    }
}
