use anyhow::{bail, Result};

pub struct Config {
    pub port: u16,
    pub db_path: String,
    pub data_dir: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_from: String,
    pub smtp_user: Option<String>,
    pub smtp_pass: Option<String>,
    /// Dev mode: skip SMTP (print OTP to stdout) and skip Bearer-token
    /// check on /api/sync. Toggle with `YOMERU_DEV_MODE=1` or `--dev-mode`.
    pub dev_mode: bool,
}

impl Config {
    pub fn from_args() -> Result<Self> {
        let args: Vec<String> = std::env::args().collect();

        fn cli_flag(args: &[String], name: &str) -> Option<String> {
            args.windows(2)
                .find(|w| w.first().map(String::as_str) == Some(name))
                .and_then(|w| w.get(1).cloned())
        }

        // CLI flag wins; env var is the fallback. Empty strings are treated as unset.
        fn resolve(args: &[String], cli: &str, env: &str) -> Option<String> {
            cli_flag(args, cli)
                .or_else(|| std::env::var(env).ok())
                .filter(|s| !s.is_empty())
        }

        let dev_mode = cli_flag(&args, "--dev-mode").is_some()
            || matches!(
                std::env::var("YOMERU_DEV_MODE").as_deref(),
                Ok("1" | "true" | "TRUE" | "yes")
            );

        // SMTP is only required when dev_mode is off — in dev we short-circuit
        // OTP delivery and never reach lettre.
        let smtp_host = resolve(&args, "--smtp-host", "YOMERU_SMTP_HOST");
        let smtp_from = resolve(&args, "--smtp-from", "YOMERU_SMTP_FROM");
        if !dev_mode {
            if smtp_host.is_none() {
                bail!("smtp host required (--smtp-host or YOMERU_SMTP_HOST)");
            }
            if smtp_from.is_none() {
                bail!("smtp from required (--smtp-from or YOMERU_SMTP_FROM)");
            }
        }

        Ok(Self {
            port: resolve(&args, "--port", "YOMERU_PORT")
                .and_then(|s| s.parse().ok())
                .unwrap_or(8080),
            db_path: resolve(&args, "--db", "YOMERU_DB_PATH")
                .unwrap_or_else(|| "./yomeru.db".into()),
            data_dir: resolve(&args, "--data-dir", "YOMERU_DATA_DIR")
                .unwrap_or_else(|| "./data".into()),
            smtp_host: smtp_host.unwrap_or_default(),
            smtp_port: resolve(&args, "--smtp-port", "YOMERU_SMTP_PORT")
                .and_then(|s| s.parse().ok())
                .unwrap_or(587),
            smtp_from: smtp_from.unwrap_or_default(),
            smtp_user: resolve(&args, "--smtp-user", "YOMERU_SMTP_USER"),
            smtp_pass: resolve(&args, "--smtp-pass", "YOMERU_SMTP_PASS"),
            dev_mode,
        })
    }
}
