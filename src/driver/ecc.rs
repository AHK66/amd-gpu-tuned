/// UMC ECC counters parsed from the RAS `umc_err_count` text file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EccCounters {
    pub ce: u64,
    pub ue: u64,
    /// `None` when the file has no `de` line.
    pub deferred: Option<u64>,
}

/// Parse UMC ECC counters from the text content of `umc_err_count`.
/// The file format is `key: value` lines (`ce`, `ue`, `de`).
/// Returns `None` when `ce`/`ue` is missing or any recognized value is
/// unparseable. Unknown keys are ignored entirely (forward compatibility),
/// so a non-numeric unknown line never invalidates the readout.
pub fn parse_umc_err_count(text: &str) -> Option<EccCounters> {
    let mut ce: Option<u64> = None;
    let mut ue: Option<u64> = None;
    let mut deferred: Option<u64> = None;
    for line in text.lines() {
        if let Some((key, val)) = line.split_once(':') {
            match key.trim() {
                "ce" => ce = Some(val.trim().parse().ok()?),
                "ue" => ue = Some(val.trim().parse().ok()?),
                "de" => deferred = Some(val.trim().parse().ok()?),
                _ => {}
            }
        }
    }
    Some(EccCounters {
        ce: ce?,
        ue: ue?,
        deferred,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_umc_err_count_all_zero() {
        let c = parse_umc_err_count("ue: 0\nce: 0\nde: 0\n").unwrap();
        assert_eq!(c.ce, 0);
        assert_eq!(c.ue, 0);
        assert_eq!(c.deferred, Some(0));
    }

    #[test]
    fn parse_umc_err_count_nonzero() {
        let c = parse_umc_err_count("ue: 5\nce: 42\nde: 3\n").unwrap();
        assert_eq!(c.ce, 42);
        assert_eq!(c.ue, 5);
        assert_eq!(c.deferred, Some(3));
    }

    #[test]
    fn parse_umc_err_count_missing_de_is_none_deferred() {
        let c = parse_umc_err_count("ue: 0\nce: 7\n").unwrap();
        assert_eq!(c.ce, 7);
        assert_eq!(c.ue, 0);
        assert_eq!(c.deferred, None);
    }

    #[test]
    fn parse_umc_err_count_missing_ce_is_none() {
        assert!(parse_umc_err_count("ue: 0\nde: 0\n").is_none());
    }

    #[test]
    fn parse_umc_err_count_missing_ue_is_none() {
        assert!(parse_umc_err_count("ce: 0\nde: 0\n").is_none());
    }

    #[test]
    fn parse_umc_err_count_non_numeric_is_none() {
        assert!(parse_umc_err_count("ue: 0\nce: abc\nde: 0\n").is_none());
    }

    #[test]
    fn parse_umc_err_count_empty_is_none() {
        assert!(parse_umc_err_count("").is_none());
    }

    #[test]
    fn parse_umc_err_count_ignores_unknown_keys() {
        let c = parse_umc_err_count("ue: 1\nce: 2\nde: 3\nfoo: 99\n").unwrap();
        assert_eq!(c.ce, 2);
        assert_eq!(c.ue, 1);
        assert_eq!(c.deferred, Some(3));
    }

    #[test]
    fn parse_umc_err_count_ignores_non_numeric_unknown_keys() {
        let c = parse_umc_err_count("ue: 1\nce: 2\nde: 3\nfoo: n/a\n").unwrap();
        assert_eq!(c.ce, 2);
        assert_eq!(c.ue, 1);
        assert_eq!(c.deferred, Some(3));
    }
}
