use actix_files::Files;
use actix_web::web::ServiceConfig;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

const WEB_DIR_NAME: &str = "web";
const PAGES: [&str; 4] = [
    "current_rank.html",
    "season_winrate.html",
    "session_winrate.html",
    "past_matches.html",
];

/// Returns the possible overlay folders in priority order: next to the executable, then in `cwd`.
fn candidates(exe_path: Option<&Path>, cwd: &Path) -> Vec<PathBuf> {
    let exe_candidate = exe_path
        .and_then(Path::parent)
        .map(|dir| dir.join(WEB_DIR_NAME));
    exe_candidate
        .into_iter()
        .chain(std::iter::once(cwd.join(WEB_DIR_NAME)))
        .collect()
}

fn first_existing_dir(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|path| path.is_dir()).cloned()
}

fn join_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Finds the overlay folder for the running process and logs an error if it is missing.
pub fn resolve() -> Option<PathBuf> {
    let exe_path = std::env::current_exe().ok();
    let cwd = std::env::current_dir().unwrap_or_default();
    let candidates = candidates(exe_path.as_deref(), &cwd);
    let found = first_existing_dir(&candidates);
    if found.is_none() {
        tracing::error!(
            "Cannot find the overlay folder. Checked these paths: {}. The overlay pages are not available.",
            join_paths(&candidates)
        );
    }
    found
}

pub fn log_page_urls(web_dir: &Path, address: SocketAddr) {
    tracing::info!("Serving overlay files from {}", web_dir.display());
    for page in PAGES {
        tracing::info!("Overlay page: http://{}/{}", address, page);
    }
}

/// Registers the static file service. Register it after all other routes, because it matches every path.
pub fn configure(cfg: &mut ServiceConfig, web_dir: Option<&Path>) {
    let Some(dir) = web_dir else {
        return;
    };
    cfg.service(Files::new("/", dir));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn unique_temp_dir() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("lolobs-web-dir-{}-{}", std::process::id(), id));
        // A process ID can repeat between runs (always under wine), so a folder from an earlier run can exist.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn candidates_lists_exe_dir_before_cwd() {
        let exe = Path::new("app").join("LoLobs.exe");
        let cwd = Path::new("work");
        let result = candidates(Some(&exe), cwd);
        assert_eq!(
            result,
            vec![Path::new("app").join("web"), Path::new("work").join("web")]
        );
    }

    #[test]
    fn candidates_without_exe_path_uses_cwd_only() {
        let cwd = Path::new("work");
        assert_eq!(candidates(None, cwd), vec![cwd.join("web")]);
    }

    #[test]
    fn prefers_web_dir_next_to_exe() {
        let exe_dir = unique_temp_dir();
        let cwd = unique_temp_dir();
        std::fs::create_dir_all(exe_dir.join("web")).expect("create exe web dir");
        std::fs::create_dir_all(cwd.join("web")).expect("create cwd web dir");
        let found = first_existing_dir(&candidates(Some(&exe_dir.join("LoLobs.exe")), &cwd));
        assert_eq!(found, Some(exe_dir.join("web")));
    }

    #[test]
    fn falls_back_to_web_dir_in_cwd() {
        let exe_dir = unique_temp_dir();
        let cwd = unique_temp_dir();
        std::fs::create_dir_all(cwd.join("web")).expect("create cwd web dir");
        let found = first_existing_dir(&candidates(Some(&exe_dir.join("LoLobs.exe")), &cwd));
        assert_eq!(found, Some(cwd.join("web")));
    }

    #[test]
    fn returns_none_when_no_web_dir_exists() {
        let exe_dir = unique_temp_dir();
        let cwd = unique_temp_dir();
        let found = first_existing_dir(&candidates(Some(&exe_dir.join("LoLobs.exe")), &cwd));
        assert_eq!(found, None);
    }

    #[test]
    fn ignores_web_file_that_is_not_a_dir() {
        let exe_dir = unique_temp_dir();
        let cwd = unique_temp_dir();
        std::fs::write(exe_dir.join("web"), b"").expect("create web file");
        std::fs::create_dir_all(cwd.join("web")).expect("create cwd web dir");
        let found = first_existing_dir(&candidates(Some(&exe_dir.join("LoLobs.exe")), &cwd));
        assert_eq!(found, Some(cwd.join("web")));
    }
}
