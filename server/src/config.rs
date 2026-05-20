pub struct Config {
    pub port: u16,
    pub db_path: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_from: String,
    pub smtp_user: Option<String>,
    pub smtp_pass: Option<String>,
}

impl Config {
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();

        fn flag(args: &[String], name: &str) -> Option<String> {
            args.windows(2)
                .find(|w| w[0] == name)
                .map(|w| w[1].clone())
        }

        Self {
            port: flag(&args, "--port").and_then(|s| s.parse().ok()).unwrap_or(8080),
            db_path: flag(&args, "--db").unwrap_or_else(|| "./yomeru.db".into()),
            smtp_host: flag(&args, "--smtp-host").expect("--smtp-host is required"),
            smtp_port: flag(&args, "--smtp-port")
                .and_then(|s| s.parse().ok())
                .unwrap_or(587),
            smtp_from: flag(&args, "--smtp-from").expect("--smtp-from is required"),
            smtp_user: flag(&args, "--smtp-user"),
            smtp_pass: flag(&args, "--smtp-pass"),
        }
    }
}
