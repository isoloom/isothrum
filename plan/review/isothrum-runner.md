# Review: isothrum-runner (Phase 1c)

The runner: it executes the run manifest Isoloom writes next to a target's files (lifecycle,
state and locks, the Compose, Vagrant and Terraform drivers, checks, readiness, the macOS relays,
the event stream), behind ports an embedder replaces. One line per 3b principle and 3d item: `ok`,
or the finding and its fix commit.

## 3b design rules
- Functional core, imperative shell: ok (`executor::plan` is the executor's pure core: whether a step applies on this host and provider, how long it may run there, a command's program and filled arguments; `vars` fills placeholders; the drivers' readers (`compose::config`, `compose::ps`, `vagrant::status`, `terraform::state`) parse the engines' answers without running anything; the effects sit in `executor::step`, the drivers' actions, `store` and `process`).
- Ports and adapters: ok (`ProcessRunner`, `HostPorts`, `Clock`, `Events` and `Hooks` are the ports, `SystemProcesses`, `LoopbackPorts`, `SystemClock`, `JsonLines` and `Recorder` the adapters; the drivers are a closed, statically dispatched set behind `Driver`; tests run on the `fake` feature's doubles, never a real tool).
- One-way dependencies: ok (isoloom-manifest only from Isoloom; no UI, no compiler: the xtask's `workspace` test).
- Parse, don't validate: ok (the manifest is Isoloom's typed contract, read once by `load_manifest`; `RunName`, `SecretValue`, `Park`, `Op`, `RunState` are types; warning codes are `WarningCode`, as failures are `ErrorCode`, where they were strings; the proxy's body length is a `u64` read once).
- Typed errors: ok (one `Error` in `error.rs`, was `RunnerError`, with its stable `ErrorCode` and `#[source]` chains: the manifest's own error, the OS's).
- No hidden state: ok (time, ports, processes and the environment come through the ports and `RunnerConfig`; the temporary-file counter of `store::fs` is the only static, a uniqueness counter with no meaning).
- Determinism: ok (ordered maps in records and events; `planning is deterministic` is a fuzz property).
- One way to do each thing: ok (one stderr tail, `process::Tail`, for every call and capture; one command-line form for a spec and a fake's call; one refusal of another driver's action and one status report for the drivers; one pump for each way of a relayed connection; one rule naming a probe's failure after its tool; the test fixtures in one `testing` module).

## 3b principles
- Names, one vocabulary, no stutter: ok (`Error`, not `RunnerError`; `executor::plan`, `process::Tail`; the 3b words run, lab, machine, driver, manifest).
- Small functions: ok (all under 60 lines, complexity under 15).
- Few arguments, no flag arguments: ok.
- Single responsibility, small modules: ok (one concept per module, every `mod.rs` now `foo.rs`; every file under 400 lines of code, `driver/vagrant/tests.rs` a test file).
- Open/closed, Liskov, interface segregation, dependency inversion: ok (small ports; the drivers implement only their actions and readers).
- DRY: ok (no `dup/clone`: the seven duplicated blocks became the shared helpers above).
- KISS and YAGNI: ok.
- Command-query separation, no hidden side effects: ok.
- Law of Demeter: ok.
- Errors as types, no null: ok (no `unwrap`, `expect`, `panic`, indexing or string slicing outside tests; nothing ignored: a write that fails, a clean-up that fails, a connection cut short are traced or reported, `check_cleanup_failed` among the warnings).
- Clean boundaries with third-party code: ok (`process-wrap` behind `SystemProcesses`, `nc` behind the relays, JSON and YAML behind each driver's reader).
- Clean tests (F.I.R.S.T., one concept each): ok (fake processes and a paused Tokio clock; no sleep, no network; integration tests return a `Result`).
- Comments say why: ok.
- Formatting: ok.
- No dead code: ok.

## 3d Rust practices
- Workspace hygiene: ok (proptest from the workspace, as every dependency).
- Common traits: ok (the public types derive `Debug`, `Clone`, `PartialEq`/`Eq` where they hold data; `Send`/`Sync` asserted in `tests/threads.rs`, an operation's future `Send`).
- Conversions through `From`: ok (`Exit` from an exit code, `HookError` from text, `Error` from the manifest's).
- Types carry meaning, `#[non_exhaustive]`: ok (40 public types and enums non-exhaustive; `WarningCode` too).
- Ownership: ok.
- `#[must_use]`: ok (the spec's and the call's command lines added).
- Visibility: ok (`pub(crate)` everywhere but the API; `process` is a public module for `capture`, `Captured`, `Exit`, `OnLine` and the `fake` doubles).
- Library errors with `thiserror` and a `Result` alias: ok.
- Error messages: ok (a driver refusing another's action says the step, the action and the driver).
- No panics in library code: ok.
- Sync or async, observability: ok (async on Tokio, chosen once; every tool call under a time limit; `tracing` spans `run`, `step`, `driver`; nothing printed).
- `unsafe`: ok (`unsafe_code` forbidden; elevation through PowerShell, process groups through `process-wrap`).
- Crate docs with a runnable example, `# Errors` and `# Panics`: ok.
- Docs build clean: ok.
- Behaviour-named tests, one concept each: ok.
- Property tests for parsers and quoting: ok (placeholder filling, the HTTP proxy's heads, the SSH quoting and logins, the Compose override's YAML quoting (it reads back as itself), the Terraform state reader, every Compose reader).
- Snapshots only for whole files: ok (none).
- Mutation testing: closed, see below.
- Fuzzing: ok, see below.

## Mutation testing (`cargo run -p xtask -- mutants isothrum-runner`, lints capped, 300 s timeout floor)
- Scope: the executor (`executor.rs`, `executor/**`) and the drivers' pure parts, what reads the engines' answers (`compose::config`, `compose::ps`, `vagrant::status`, `terraform::state`). The drivers' actions and the lab's operations are the imperative shell, tested on fakes but not mutated.
- Before (on 2e0062e, the reorganized crate): 199 mutants, 154 caught, 13 missed, 1 timeout, 31 unviable. The timeout was `step::call` replaced by a default answer: the Terraform lab tests then waited on the real clock for a state that never came.
- Fixed in a384062, by tests only (no survivor was dead code or an equivalent mutant): a `map_entry` slot is one key of its variable's JSON object; a verdict cut to its limit is exactly that long; a driver's call is logged only when shown; a Compose port without its target makes the config unreadable; a publisher to or from port 0 is not published, one listed twice is kept once; a published port is named by its port and its transport both; a machine is found by its Compose service, not its name; a Terraform state that exists but can't be read is an error, a boolean output reads `true`; the first named Vagrant provider stays. The Terraform lab tests run on a paused clock, so the timeout became a quick failure.
- Closure: the 14 survivors rerun on the final code (`cargo mutants --iterate`), 14 caught. Then the gate itself on the final code (`cargo run -p xtask -- mutants isothrum-runner`, a384062): 199 mutants, 168 caught, 0 missed, 0 timeouts, 31 unviable (a replacement value the type can't take).
- Nothing is listed in `.cargo/mutants.toml`.

## Fuzzing
- `cargo run -p xtask -- fuzz --time 300 plan`: 492,759 runs in 301 s, no crash. Every step of any manifest (the runner's fixtures as seeds) is planned on every host and provider (elsewhere, a command, an action, or refused with why), never a panic, the same twice; with every run value bound, no step is refused for a `${run.…}` one.
- `cargo run -p xtask -- fuzz --time 300 placeholders`: 470,650 runs in 301 s, no crash. Any template filled with any values is filled or names the placeholder without a value; filled whole, it reads as a lab's message does; a value goes in as it is, never read as a placeholder. Its first run found the target's property wrong, not the runner: a param named `a}b` makes `${params.a}b}` param `a`'s placeholder followed by text; the property now holds for a name whose placeholder parses as that one alone (3c65ca6).

## 1c organization
- `lib.rs` holds only the module tree and the re-exported API: ok (`BoxFuture` in `future`, the thread tests in `tests/threads.rs`).
- Modules by domain concept: ok.
- One `Error` in `error.rs` with a `Result` alias: ok.
- Tests next to the code, integration tests in `tests/`, fixtures in `tests/fixtures/`, shared helpers in `testing`: ok.
