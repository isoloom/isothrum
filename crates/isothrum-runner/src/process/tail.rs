//! The last lines a tool printed on standard error: what leads its error when it fails.

use std::collections::VecDeque;

/// How many standard error lines are kept.
const KEPT: usize = 10;

/// A tool's last [`KEPT`] standard error lines, a Ruby backtrace's frames left out: what leads its
/// error when it fails.
#[derive(Debug, Default)]
pub(crate) struct Tail(VecDeque<String>);

impl Tail {
    /// Keeps `line`, the oldest dropped past [`KEPT`]; a backtrace frame isn't kept.
    pub(crate) fn push(&mut self, line: &str) {
        if is_backtrace_frame(line) {
            return;
        }
        if self.0.len() == KEPT {
            self.0.pop_front();
        }
        self.0.push_back(line.to_owned());
    }

    /// The lines kept, oldest first.
    pub(crate) fn into_lines(self) -> Vec<String> {
        self.0.into()
    }
}

/// Whether `line` is a frame of a Ruby backtrace (`\tfrom /…/builder.rb:180:in 'call'`), which
/// never counts among a failure's last lines: Vagrant prints its error, then dozens of frames.
fn is_backtrace_frame(line: &str) -> bool {
    line.trim_start()
        .strip_prefix("from ")
        .is_some_and(|rest| rest.contains(":in `") || rest.contains(":in '"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ruby_backtraces_frames_arent_a_failures_last_lines() {
        assert!(is_backtrace_frame("\tfrom /usr/lib/vagrant/action/builder.rb:180:in `call'"));
        assert!(is_backtrace_frame("    from <internal:kernel>:187:in 'loop'"));
        assert!(!is_backtrace_frame("wait_for.rb:9:in `block in wait_for': timeout (Fog::Errors::TimeoutError)"));
        assert!(!is_backtrace_frame("from here on, the VM is gone"));
    }

    #[test]
    fn a_tail_keeps_the_last_lines_without_backtrace_frames() {
        let mut tail = Tail::default();
        for i in 0..=KEPT {
            tail.push(&format!("line {i}"));
        }
        tail.push("\tfrom /usr/lib/vagrant/action/builder.rb:180:in `call'");
        let lines = tail.into_lines();
        assert_eq!(lines.len(), KEPT);
        assert_eq!(lines.first().map(String::as_str), Some("line 1"), "the oldest dropped");
        assert_eq!(lines.last().map(String::as_str), Some("line 10"), "the frame not kept");
    }
}
