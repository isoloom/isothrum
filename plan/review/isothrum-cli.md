# Review: isothrum-cli (Phase 1c)

`isothrum`: the runner from the command line (up, down, park, resume, status, check, provision,
events, key). The imperative shell around the runner. One line per 3b principle and 3d item:
`ok`, or the finding and its fix commit.

## 3b design rules
- Functional core, imperative shell: ok (the environment and the arguments are read once in `main`; `commands` takes the runner, the terminal and the environment as values; the text it prints is built by pure functions in `text`).
- Ports and adapters: ok (the runner's ports: `SystemProcesses` in `main`, fake processes and ports in the tests).
- One-way dependencies: ok (the runner only; never the compiler or a UI: the xtask's `workspace` test).
- Parse, don't validate: ok (params and slots parsed once in `values`; the park mode is a `Mode`, matched exhaustively; the outcome a `Verdict`, not a `bool`).
- Typed errors: ok (one `Error` in `error.rs`, was `CliError`; the runner's errors say their cause in their message, so one line is printed).
- No hidden state: ok (`ISOTHRUM_HOME` and the slots' variables are read through the `var` function passed in).
- Determinism: ok.
- One way to do each thing: ok (exit codes in `exit` only; every write through `Term`).

## 3b principles
- Names, one vocabulary, no stutter: ok.
- Small functions: ok.
- Few arguments, no flag arguments: ok (`status`/`check`/`print_events` take `json` from `--json`, the flag the user gave).
- Single responsibility, small modules: ok (`args`, `commands`, `locate`, `values`, `term`, `text`, `exit`, `error`).
- Open/closed, Liskov, interface segregation, dependency inversion: ok.
- DRY: ok (one `testing::Buf` for the tests' outputs).
- KISS and YAGNI: ok.
- Command-query separation, no hidden side effects: ok.
- Law of Demeter: ok.
- Errors as types, no null: ok (no ignored result left: a failed write is kept and said once the run is done; a closed pipe is no error).
- Clean boundaries with third-party code: ok (clap behind `args`, tracing-subscriber in `main`).
- Clean tests (F.I.R.S.T., one concept each): ok (the commands end to end on a scripted Compose engine; the real run is ignored and returns a `Result`, its record printed by the test).
- Comments say why: ok.
- Formatting: ok.
- No dead code: ok.

## 3d Rust practices
- Workspace hygiene: ok.
- Common traits: ok (private types; `Verdict`: `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`).
- Conversions through `From`: ok (`Error` from the runner's and serde's).
- Types carry meaning: ok (`Verdict`, `Mode`).
- Ownership: ok.
- `#[must_use]`: ok.
- Visibility: ok (a binary: everything `pub(crate)`).
- Errors: ok (`anyhow`-free; a `Box<dyn Error>` only for the tracing set-up's two error kinds, in the binary).
- Error messages: ok.
- No panics: ok.
- Sync or async, observability: ok (async on Tokio, as the runner; `ISOTHRUM_LOG` sets the tracing level, a wrong one is said as a warning).
- Crate docs, `# Errors`: ok.
- Docs build clean: ok.
- Behaviour-named tests, one concept each: ok.
- Property tests for parsers: ok (`--param NAME=VALUE`).
- Snapshots: ok (none).
- Mutation testing: not in scope (the shell; the runner's executor is mutated).
- Fuzzing: not in scope (its parsing is clap's and `values`, property-tested).

## Found on the way
- A write that failed was dropped (`let _ = writeln!`): now the first one is kept and said once the command is done (exit 1), while a reader that left is no error. Tested with failing writers.
- An `ISOTHRUM_LOG` value that isn't a level was ignored silently: now a warning.

## 1c organization
- `main.rs` holds the module tree, `main` and the runner's construction: ok.
- Modules by domain concept: ok.
- One `Error` in `error.rs`, exit codes in `exit`: ok.
- Tests next to the code (`commands/tests.rs`), integration tests in `tests/`, shared helpers in `testing`: ok.
