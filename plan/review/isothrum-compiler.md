# Review: isothrum-compiler (Phase 1c)

A lab as drawn in the editor (a `Topology`: machines, LANs, cables) becomes an Isoloom spec, read
back by Isoloom's parser, with the lab's own files (appliance configurations, starter scripts).
One line per 3b principle and 3d item: `ok`, or the finding and its fix commit.

## 3b design rules
- Functional core, imperative shell: ok (pure over the drawing: no I/O, clock or environment; the WASM engine and the desktop app are its shells).
- Ports and adapters: ok (nothing outside to reach; Isoloom's parser is called as a library).
- One-way dependencies: ok (isoloom-spec only; the WASM engine and the desktop app depend on it; the xtask's `workspace` test keeps it off the runner, Tokio and every UI).
- Parse, don't validate: ok (the drawing is read once by serde into typed nodes: an `OsRef`, a `PortProtocol`, an `Appliance`, a `NonZeroU16` port, `Kind` instead of strings; a LAN's block stays the text drawn, since judging it is Isoloom's, and the compiler reads only its prefix length and, for a switch, the block covering its VLANs).
- Typed errors: ok (one `Error` in `error.rs`, was `CompileError`; every variant names the drawing's node, LAN, cable or port; `PortName` and `Yaml` carry their `#[source]`).
- No hidden state: ok.
- Determinism: ok (`IndexMap` in the drawing's order everywhere; the fixtures' snapshots and the properties check it).
- One way to do each thing: ok (one starter `add` rule for every path; the cables' block said once, `LINK_BLOCK`, the error message included; a network's internet written by `NetworkDoc` alone).

## 3b principles
- Names, one vocabulary, no stutter: ok (`addressing` split into `lans` and `cables`; `Error`, not `CompileError`; `machines::add`, `lans::address`, `cables::allocate`, `vlans::nest`, the steps of `compile` in its order).
- Small functions: ok (all under 60 lines, complexity under 15).
- Few arguments, no flag arguments: ok (`NetworkDoc::new`'s `internet` is the network's own fact, not a mode).
- Single responsibility, small modules: ok (`topology`, `machines`, `ports`, `controller`, `lans`, `cables`, `routers`, `vlans`, `draft`, `document`, `compiled`, `project`, `starters` with `packages`, `error`; every file under 400 lines of code).
- Open/closed, Liskov, interface segregation, dependency inversion: ok.
- DRY: ok (no `dup/clone`).
- KISS and YAGNI: ok (`config_files`, `starters`, `vlan_ref`, `config_path` and `firmware_path` left the public API; the lab's files come with `Compiled`).
- Command-query separation, no hidden side effects: ok.
- Law of Demeter: ok.
- Errors as types, no null: ok (no `unwrap`, `expect`, `panic`, indexing or string slicing outside tests).
- Clean boundaries with third-party code: ok (serde YAML behind `document`; Isoloom's types read, never written: the compiler writes its own documents).
- Clean tests (F.I.R.S.T., one concept each): ok (unit tests next to the code; one integration binary, `tests/compile/`, by concept, with one `testing` module; tests return a `Result` instead of allowing `unwrap`).
- Comments say why: ok.
- Formatting: ok (160 columns, as the other crates).
- No dead code: ok (`unused` at deny; machete clean).

## 3d Rust practices
- Workspace hygiene: ok (every field, lint and dependency from the workspace; the xtask test checks it).
- Common traits: ok (`Topology`, `Lan`, `Node`, `Link`: `Debug`, `Clone`, `PartialEq`, serde; `Service`, `Endpoint` also `Eq`, `Hash`; `Compiled`: `Debug`, `Clone`, `PartialEq`; `LabFile`: `Debug`, `Clone`, `PartialEq`, `Eq`; `Error`: `Debug`, `Display`, `std::error::Error`).
- Conversions through `From`: ok (`ProvisionDoc` from a controller playbook; serde for the drawing).
- Types carry meaning, `#[non_exhaustive]`: ok (`Error` and `Kind` are non-exhaustive; the drawing's structs stay open on purpose: they are the editor's JSON, built by callers and tests, and a new field is a deliberate change of that contract).
- Ownership: ok.
- `#[must_use]`: ok.
- Visibility: ok (`pub(crate)` everywhere but the API: `compile`, `Compiled`, `LabFile`, the drawing's types, `Error`, `Result`).
- Library errors with `thiserror` and a `Result` alias: ok.
- Error messages: ok (lowercase but for names; no trailing period; each says the fix where there is one).
- No panics in library code: ok.
- Sync or async, observability: ok (synchronous and pure; it logs nothing).
- Crate docs with a runnable example, `# Errors` and `# Panics`: ok (the crate example compiles a drawing; `compile` has `# Errors`).
- Docs build clean: ok.
- Behaviour-named tests, one concept each: ok.
- Property tests for parsers: ok (well-formed drawings compile to specs Isoloom accepts; any drawing compiles or says what's wrong with the drawing; port names; the regression seed of a Windows playbook kept).
- Snapshots only for whole files: ok (the five fixtures' specs).
- Mutation testing: closed, see below.
- Fuzzing: ok, see below.

## Isoloom's validation (4bfd8d5f), followed
- A Windows node's playbooks run from Isoloom's controller (`provision:`), its scripts stay its own steps; the playbook's starter sets up that machine over WinRM.
- A drawn Linux VM that installs a service (ports, steps, no command) has no container form: the office and WAN fixtures generate for hybrid, and the desktop app falls back from Docker (its test covers hybrid and Vagrant). The compiler invents no command.

## Mutation testing (`cargo xtask mutants isothrum-compiler`, lints capped, 300 s timeout floor)
- Before: 262 mutants, 9 missed, 1 timeout. The timeout was a real unbounded loop: the free port name was searched with `n += 1` until one was free, and a mutant made it endless.
- Fixed in 7a8d19f: the free name is one of the first `len + 1` names (a machine's ports take at most `len`), a bounded search; a block's usable addresses come from its host bits, without a conversion whose fallback never ran; the cables' blocks are zipped with the cables, without an error `check_count` had already ruled out. New tests: the message of a spec Isoloom doesn't read, a blank appliance configuration (no file), a starter's services line, MariaDB's and Redis' listening, a package two ports call for installed once, every block's usable range, the cables' blocks in order.
- After, on the landed code: 262 mutants, 237 caught, 0 missed, 0 timeouts, 25 unviable (a replacement value the type can't take). Nothing is listed in `.cargo/mutants.toml`.

## Fuzzing
- `cargo run -p xtask -- fuzz --time 300 drawing`: 4,025,000 runs in 301 s, no crash. The target compiles any drawing the editor could send (the compiler's fixtures as seeds): a drawing compiles or is refused for what it is, never for what the compiler wrote (a spec Isoloom doesn't read, a port name, YAML); the lab's own files stay inside the project and out of Isoloom's, each once, each ending with a line break.

## 1c organization
- `lib.rs` holds only the module tree and the re-exported API: ok.
- Modules by domain concept: ok.
- One `Error` in `error.rs` with a `Result` alias: ok.
- Tests next to the code, integration tests in `tests/`, fixtures in `tests/fixtures/`, shared helpers in `testing`: ok.
