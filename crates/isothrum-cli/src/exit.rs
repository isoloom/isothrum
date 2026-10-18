//! The command line's exit codes, worked out in one place: 0 when a command reached its goal; 1
//! when it didn't (an operation that failed, a failing check, the runner refusing); 2 for a
//! usage error (a run with no manifest, a value missing), clap's own included. Output whose
//! reader stopped reading never changes the code: the run it reports went on.

use std::process::ExitCode;

use crate::error::{Error, Result};

/// What a command that ran to its end found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The command reached its goal: the run is up, every check passed.
    Pass,
    /// It didn't, and its events say why.
    Fail,
}

impl Verdict {
    /// [`Verdict::Pass`] when `ok`, else [`Verdict::Fail`].
    pub(crate) fn of(ok: bool) -> Verdict {
        if ok { Verdict::Pass } else { Verdict::Fail }
    }
}

/// The exit code a command's outcome gives.
pub(crate) fn code(outcome: &Result<Verdict>) -> ExitCode {
    ExitCode::from(match outcome {
        Ok(Verdict::Pass) => 0,
        Err(Error::Usage(_)) => 2,
        Ok(Verdict::Fail) | Err(_) => 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_goal_reached_is_0_a_failure_1_and_a_usage_error_2() {
        assert_eq!(code(&Ok(Verdict::of(true))), ExitCode::from(0));
        assert_eq!(code(&Ok(Verdict::of(false))), ExitCode::from(1));
        assert_eq!(code(&Err(Error::Usage("no manifest".into()))), ExitCode::from(2));
        let full = Error::Output(std::io::Error::other("no space left on device"));
        assert_eq!(code(&Err(full)), ExitCode::from(1));
    }
}
