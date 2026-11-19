use crate::config::{SessionMode, next_to_exe};
use crate::lcu::{QueueId, Standing, SummonerId};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

const SESSION_FILE_NAME: &str = "session.json";
const FILE_VERSION: u32 = 1;

/// The account and the queue that a session belongs to.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionOwner {
    pub summoner_id: SummonerId,
    pub queue_id: QueueId,
}

/// The standing at the start of a session and the local date of that start.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
struct Baseline {
    #[serde(flatten)]
    owner: SessionOwner,
    date: NaiveDate,
    #[serde(flatten)]
    standing: Standing,
}

#[derive(Serialize, Deserialize)]
struct SessionFile {
    version: u32,
    #[serde(flatten)]
    baseline: Baseline,
}

#[derive(Debug)]
pub enum SessionFileError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    Version {
        path: PathBuf,
        version: u32,
    },
    Write {
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for SessionFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => write!(
                f,
                "Cannot read the session file {}: {}",
                path.display(),
                source
            ),
            Self::Parse { path, source } => write!(
                f,
                "The session file {} is not valid: {}",
                path.display(),
                source
            ),
            Self::Version { path, version } => write!(
                f,
                "The session file {} has the unsupported version {}",
                path.display(),
                version
            ),
            Self::Write { path, source } => write!(
                f,
                "Cannot write the session file {}: {}",
                path.display(),
                source
            ),
        }
    }
}

/// Returns `None` when the file does not exist.
fn read_baseline(path: &Path) -> Result<Option<Baseline>, SessionFileError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(SessionFileError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    parse_baseline(path, &text).map(Some)
}

fn parse_baseline(path: &Path, text: &str) -> Result<Baseline, SessionFileError> {
    let file: SessionFile =
        serde_json::from_str(text).map_err(|source| SessionFileError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    if file.version != FILE_VERSION {
        return Err(SessionFileError::Version {
            path: path.to_path_buf(),
            version: file.version,
        });
    }
    Ok(file.baseline)
}

fn write_baseline(path: &Path, baseline: Baseline) -> Result<(), SessionFileError> {
    let file = SessionFile {
        version: FILE_VERSION,
        baseline,
    };
    serde_json::to_string_pretty(&file)
        .map_err(io::Error::other)
        .and_then(|text| std::fs::write(path, text))
        .map_err(|source| SessionFileError::Write {
            path: path.to_path_buf(),
            source,
        })
}

/// Returns `None` when the file does not exist or is not valid. Logs a warning for a file that is not valid.
fn load_baseline(path: &Path) -> Option<Baseline> {
    read_baseline(path)
        .inspect_err(|err| {
            tracing::warn!(target: "session", "{}. The server starts a new session.", err);
        })
        .ok()
        .flatten()
}

fn initial_baseline(mode: SessionMode, path: &Path) -> Option<Baseline> {
    match mode {
        SessionMode::Resume => load_baseline(path),
        SessionMode::Reset | SessionMode::Off => None,
    }
}

/// Returns the saved standing when it belongs to `owner` and was saved on `today`.
fn reusable_standing(
    saved: Option<&Baseline>,
    owner: SessionOwner,
    today: NaiveDate,
) -> Option<Standing> {
    saved
        .filter(|baseline| baseline.owner == owner && baseline.date == today)
        .map(|baseline| baseline.standing)
}

/// The saved session baseline and the file that keeps it.
#[derive(Debug)]
pub struct SessionStore {
    path: PathBuf,
    saved: Option<Baseline>,
}

impl SessionStore {
    /// Returns `None` when `mode` is `Off` or when the folder of the executable is not known.
    pub fn open(mode: SessionMode, exe_path: Option<&Path>) -> Option<Self> {
        if mode == SessionMode::Off {
            return None;
        }
        let Some(path) = next_to_exe(exe_path, SESSION_FILE_NAME) else {
            tracing::warn!(target: "session", "Cannot find the folder of the executable. The server does not save the session.");
            return None;
        };
        let saved = initial_baseline(mode, &path);
        Some(Self { path, saved })
    }

    /// Returns the saved standing when it matches `owner` and `today`. Otherwise saves `current` as the new baseline.
    pub fn baseline(
        &mut self,
        owner: SessionOwner,
        today: NaiveDate,
        current: Standing,
    ) -> Standing {
        if let Some(standing) = reusable_standing(self.saved.as_ref(), owner, today) {
            tracing::info!(target: "session", "Continuing the saved session from {}", self.path.display());
            return standing;
        }
        self.save(Baseline {
            owner,
            date: today,
            standing: current,
        });
        current
    }

    fn save(&mut self, baseline: Baseline) {
        self.saved = Some(baseline);
        if let Err(err) = write_baseline(&self.path, baseline) {
            tracing::warn!(target: "session", "{}. The server continues without the saved session.", err);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{Division, LadderPoints, Record, Tier};
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn unique_temp_dir() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("lolobs-session-{}-{}", std::process::id(), id));
        // A process ID can repeat between runs (always under wine), so a folder from an earlier run can exist.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn exe_in(dir: &Path) -> PathBuf {
        dir.join("LoLobs.exe")
    }

    fn session_file(dir: &Path) -> PathBuf {
        dir.join(SESSION_FILE_NAME)
    }

    fn day(value: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, value).expect("date is valid")
    }

    fn owner(summoner_id: u64) -> SessionOwner {
        SessionOwner {
            summoner_id: SummonerId(summoner_id),
            queue_id: QueueId(420),
        }
    }

    fn standing(wins: i64, losses: i64, league_points: i64) -> Standing {
        Standing {
            record: Record { wins, losses },
            ladder: LadderPoints::new(
                &Tier("GOLD".to_string()),
                Some(Division::Two),
                league_points,
            ),
        }
    }

    /// Saves a baseline of `standing(10, 5, 40)` for `summoner_id` on `date` in `dir`.
    fn save_first_session(dir: &Path, summoner_id: u64, date: NaiveDate) {
        let mut store =
            SessionStore::open(SessionMode::Resume, Some(&exe_in(dir))).expect("store opens");
        store.baseline(owner(summoner_id), date, standing(10, 5, 40));
    }

    fn restart(dir: &Path, mode: SessionMode) -> SessionStore {
        SessionStore::open(mode, Some(&exe_in(dir))).expect("store opens")
    }

    #[test]
    fn same_summoner_on_same_day_reuses_the_baseline() {
        let dir = unique_temp_dir();
        save_first_session(&dir, 1, day(25));

        let mut store = restart(&dir, SessionMode::Resume);
        let baseline = store.baseline(owner(1), day(25), standing(12, 6, 70));

        assert_eq!(baseline, standing(10, 5, 40));
    }

    #[test]
    fn same_summoner_on_other_day_starts_a_new_baseline() {
        let dir = unique_temp_dir();
        save_first_session(&dir, 1, day(24));

        let mut store = restart(&dir, SessionMode::Resume);
        let baseline = store.baseline(owner(1), day(25), standing(12, 6, 70));

        assert_eq!(baseline, standing(12, 6, 70));
        assert_eq!(
            load_baseline(&session_file(&dir)).map(|b| b.date),
            Some(day(25))
        );
    }

    #[test]
    fn other_summoner_starts_a_new_baseline() {
        let dir = unique_temp_dir();
        save_first_session(&dir, 1, day(25));

        let mut store = restart(&dir, SessionMode::Resume);
        let baseline = store.baseline(owner(2), day(25), standing(12, 6, 70));

        assert_eq!(baseline, standing(12, 6, 70));
        assert_eq!(
            load_baseline(&session_file(&dir)).map(|b| b.owner),
            Some(owner(2))
        );
    }

    #[test]
    fn other_queue_starts_a_new_baseline() {
        let dir = unique_temp_dir();
        save_first_session(&dir, 1, day(25));
        let flex = SessionOwner {
            queue_id: QueueId(440),
            ..owner(1)
        };

        let mut store = restart(&dir, SessionMode::Resume);

        assert_eq!(
            store.baseline(flex, day(25), standing(12, 6, 70)),
            standing(12, 6, 70)
        );
    }

    #[test]
    fn missing_file_starts_a_new_baseline_and_writes_the_file() {
        let dir = unique_temp_dir();

        let mut store = restart(&dir, SessionMode::Resume);
        let baseline = store.baseline(owner(1), day(25), standing(12, 6, 70));

        assert_eq!(baseline, standing(12, 6, 70));
        assert_eq!(
            read_baseline(&session_file(&dir)).expect("file is valid"),
            Some(Baseline {
                owner: owner(1),
                date: day(25),
                standing: standing(12, 6, 70),
            })
        );
    }

    #[test]
    fn corrupt_file_starts_a_new_baseline() {
        let dir = unique_temp_dir();
        std::fs::write(session_file(&dir), "{ not json").expect("write file");

        let mut store = restart(&dir, SessionMode::Resume);
        let baseline = store.baseline(owner(1), day(25), standing(12, 6, 70));

        assert_eq!(baseline, standing(12, 6, 70));
        assert!(read_baseline(&session_file(&dir)).is_ok());
    }

    #[test]
    fn corrupt_file_error_names_the_path() {
        let path = unique_temp_dir().join(SESSION_FILE_NAME);
        std::fs::write(&path, "{ not json").expect("write file");

        let err = read_baseline(&path).expect_err("file is not valid");

        assert!(
            err.to_string().contains(&path.display().to_string()),
            "{err}"
        );
    }

    #[test]
    fn unsupported_version_is_an_error() {
        let path = Path::new(SESSION_FILE_NAME);
        let text = r#"{"version": 2, "summonerId": 1, "queueId": 420, "date": "2026-10-25",
            "wins": 10, "losses": 5, "ladderPoints": 1440}"#;

        let err = parse_baseline(path, text).expect_err("version is unsupported");

        assert!(err.to_string().contains("version 2"), "{err}");
    }

    #[test]
    fn file_keeps_all_baseline_fields() {
        let text = r#"{"version": 1, "summonerId": 1, "queueId": 420, "date": "2026-10-25",
            "wins": 10, "losses": 5, "ladderPoints": 1440}"#;

        let baseline = parse_baseline(Path::new(SESSION_FILE_NAME), text).expect("file parses");

        assert_eq!(
            baseline,
            Baseline {
                owner: owner(1),
                date: day(25),
                standing: standing(10, 5, 40),
            }
        );
    }

    #[test]
    fn reset_mode_ignores_and_replaces_the_saved_baseline() {
        let dir = unique_temp_dir();
        save_first_session(&dir, 1, day(25));

        let mut store = restart(&dir, SessionMode::Reset);
        let baseline = store.baseline(owner(1), day(25), standing(12, 6, 70));

        assert_eq!(baseline, standing(12, 6, 70));
        assert_eq!(
            load_baseline(&session_file(&dir)).map(|b| b.standing),
            Some(standing(12, 6, 70))
        );
    }

    #[test]
    fn off_mode_does_not_read_or_write_the_file() {
        let dir = unique_temp_dir();
        std::fs::write(session_file(&dir), "{ not json").expect("write file");

        let store = SessionStore::open(SessionMode::Off, Some(&exe_in(&dir)));

        assert!(store.is_none());
        assert_eq!(
            std::fs::read_to_string(session_file(&dir)).expect("read file"),
            "{ not json"
        );
    }

    #[test]
    fn write_failure_keeps_the_new_baseline() {
        let dir = unique_temp_dir();
        std::fs::create_dir_all(session_file(&dir)).expect("create blocking folder");

        let mut store = restart(&dir, SessionMode::Resume);
        let first = store.baseline(owner(1), day(25), standing(12, 6, 70));
        let second = store.baseline(owner(1), day(25), standing(13, 6, 90));

        assert_eq!(first, standing(12, 6, 70));
        assert_eq!(second, standing(12, 6, 70));
    }
}
