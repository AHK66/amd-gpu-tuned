use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "amd-gpu-tuned",
    about = "Direct-sysfs tuning daemon for 2x Radeon AI PRO R9700"
)]
pub struct Config {
    /// TCP port (always bound to 127.0.0.1)
    #[arg(long, global = true)]
    pub port: Option<u16>,
    /// Data directory (spec §16 `--data` = `./data`); the database is
    /// `<data>/amd-gpu-tuned.db`
    #[arg(long, default_value = "./data", global = true)]
    pub data: PathBuf,
    /// Telemetry poll interval (seconds)
    #[arg(long, default_value_t = 1.5, global = true)]
    pub poll_s: f64,
    /// Fast sub-sample interval for telemetry period aggregation (seconds).
    /// `0` disables sub-sampling (single-cadence, today's behavior).
    #[arg(long, default_value_t = 0.25, global = true)]
    pub record_s: f64,
    /// Enumerate GPUs from a captured sysfs fixture tree instead of
    /// `/sys/class/drm` (e2e tests; no hardware, no root).
    #[arg(long, global = true)]
    pub fake_sysfs: Option<PathBuf>,

    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum Cmd {
    /// Run the daemon (the default when no subcommand is given)
    Run,
    /// Store the write-endpoint password hash (or delete it with --clear)
    SetPassword {
        /// Delete the stored hash (return to open mode)
        #[arg(long)]
        clear: bool,
    },
}

impl Config {
    /// `record_s` must not exceed `poll_s` (spec §3.7): a commit that fires
    /// before any sub-sample lands would emit an empty period.
    pub fn validate_intervals(&self) -> Result<(), String> {
        if self.record_s > 0.0 && self.record_s > self.poll_s {
            Err(format!(
                "--record-s ({}) must be <= --poll-s ({})",
                self.record_s, self.poll_s
            ))
        } else {
            Ok(())
        }
    }
}

/// The CLI `--port` (if given) wins over the built-in default.
pub fn resolve_port(cli: Option<u16>, builtin: u16) -> u16 {
    cli.unwrap_or(builtin)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(poll_s: f64, record_s: f64) -> Config {
        Config {
            port: None,
            data: std::path::PathBuf::from("./data"),
            poll_s,
            fake_sysfs: None,
            record_s,
            cmd: None,
        }
    }

    #[test]
    fn validate_intervals_rejects_record_above_poll() {
        assert!(cfg(1.5, 2.0).validate_intervals().is_err());
    }

    #[test]
    fn validate_intervals_accepts_record_below_poll() {
        assert!(cfg(1.5, 0.25).validate_intervals().is_ok());
    }

    #[test]
    fn validate_intervals_accepts_zero_record() {
        assert!(cfg(1.5, 0.0).validate_intervals().is_ok());
    }

    #[test]
    fn resolve_port_cli_overrides_builtin() {
        assert_eq!(resolve_port(Some(9000), 8787), 9000);
    }

    #[test]
    fn resolve_port_falls_back_to_builtin() {
        assert_eq!(resolve_port(None, 8787), 8787);
        assert_eq!(resolve_port(None, 9999), 9999);
    }

    #[test]
    fn parses_bare_invocation_as_no_subcommand() {
        let c = Config::try_parse_from(["amd-gpu-tuned"]).unwrap();
        assert!(c.cmd.is_none());
    }

    #[test]
    fn parses_flag_only_invocation() {
        // Existing launch style must keep working (no subcommand word).
        let c = Config::try_parse_from(["amd-gpu-tuned", "--port", "9000", "--data", "x"])
            .unwrap();
        assert!(c.cmd.is_none());
        assert_eq!(c.port, Some(9000));
        assert_eq!(c.data, std::path::PathBuf::from("x"));
    }

    #[test]
    fn parses_set_password_with_global_flag_after_subcommand() {
        let c = Config::try_parse_from(["amd-gpu-tuned", "set-password", "--data", "x"])
            .unwrap();
        assert!(matches!(c.cmd, Some(Cmd::SetPassword { clear: false })));
        assert_eq!(c.data, std::path::PathBuf::from("x"));
    }

    #[test]
    fn parses_set_password_clear_with_top_level_flag() {
        let c =
            Config::try_parse_from(["amd-gpu-tuned", "--port", "9000", "set-password", "--clear"])
                .unwrap();
        assert!(matches!(c.cmd, Some(Cmd::SetPassword { clear: true })));
        assert_eq!(c.port, Some(9000));
    }
}
