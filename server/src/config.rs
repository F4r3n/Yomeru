pub struct Config {
    pub port: u16,
    pub db_path: String,
    pub data_dir: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_from: String,
    pub smtp_user: Option<String>,
    pub smtp_pass: Option<String>,
}

impl Config {
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();

        fn cli_flag(args: &[String], name: &str) -> Option<String> {
            args.windows(2)
                .find(|w| w[0] == name)
                .map(|w| w[1].clone())
        }

        // CLI flag wins; env var is the fallback. Empty strings are treated as unset.
        fn resolve(args: &[String], cli: &str, env: &str) -> Option<String> {
            cli_flag(args, cli)
                .or_else(|| std::env::var(env).ok())
                .filter(|s| !s.is_empty())
        }

        Self {
            port: resolve(&args, "--port", "YOMERU_PORT")
                .and_then(|s| s.parse().ok())
                .unwrap_or(8080),
            db_path: resolve(&args, "--db", "YOMERU_DB_PATH")
                .unwrap_or_else(|| "./yomeru.db".into()),
            data_dir: resolve(&args, "--data-dir", "YOMERU_DATA_DIR")
                .unwrap_or_else(|| "./data".into()),
            smtp_host: resolve(&args, "--smtp-host", "YOMERU_SMTP_HOST")
                .expect("smtp host required (--smtp-host or YOMERU_SMTP_HOST)"),
            smtp_port: resolve(&args, "--smtp-port", "YOMERU_SMTP_PORT")
                .and_then(|s| s.parse().ok())
                .unwrap_or(587),
            smtp_from: resolve(&args, "--smtp-from", "YOMERU_SMTP_FROM")
                .expect("smtp from required (--smtp-from or YOMERU_SMTP_FROM)"),
            smtp_user: resolve(&args, "--smtp-user", "YOMERU_SMTP_USER"),
            smtp_pass: resolve(&args, "--smtp-pass", "YOMERU_SMTP_PASS"),
        }
    }
}
