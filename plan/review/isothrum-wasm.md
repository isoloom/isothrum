# Review: isothrum-wasm (Phase 1c)

The editor's engine in WebAssembly: a topology (the editor's JSON) becomes the plan the editor
shows (the spec, Isoloom's problems, every target's files, the machines' addresses), the lab's
own files, and the zips to download. One line per 3b principle and 3d item: `ok`, or the finding
and its fix commit.

## 3b design rules
- Functional core, imperative shell: ok (pure: JSON in, plan, files or bytes out; the browser and the website's server are its shells).
- Ports and adapters: ok (`js` is the adapter to JavaScript; the Rust API under it is the same for Rust callers and tests).
- One-way dependencies: ok (the compiler and Isoloom only; never the runner, Tokio or a process: the xtask's `workspace` test).
- Parse, don't validate: ok (the topology is parsed once, into the compiler's types; Isoloom's problems come from Isoloom).
- Typed errors: ok (one `Error` in `error.rs`, was `EngineError`; its message is what the editor shows, so a variant with a cause says it too: the JavaScript side reads one line).
- No hidden state: ok.
- Determinism: ok (files in Isoloom's order; zip entries in the plan's order, the spec first).
- One way to do each thing: ok (the lab's own files come from the compiler's `Compiled::files`; the `pair` topology shared by both test binaries).

## 3b principles
- Names, one vocabulary, no stutter: ok (`Error`, not `EngineError`; `File` is a file the engine hands out).
- Small functions: ok.
- Few arguments, no flag arguments: ok.
- Single responsibility, small modules: ok (`plan`, `bundle`, `files`, `choices`, `js`, `error`).
- Open/closed, Liskov, interface segregation, dependency inversion: ok.
- DRY: ok (the two duplicated blocks gone: the shared topology fixture, a `clean_plan` helper).
- KISS and YAGNI: ok.
- Command-query separation, no hidden side effects: ok.
- Law of Demeter: ok.
- Errors as types, no null: ok (`Plan`'s `ok`/`error` pair is the editor's JSON contract, unchanged).
- Clean boundaries with third-party code: ok (`zip` behind `bundle`, `wasm-bindgen` behind `js`).
- Clean tests (F.I.R.S.T., one concept each): ok (native tests in `tests/engine.rs`, the browser's in `tests/exports.rs`, both on `tests/fixtures/pair.json`; tests return a `Result`).
- Comments say why: ok.
- Formatting: ok.
- No dead code: ok.

## 3d Rust practices
- Workspace hygiene: ok (`wasm-bindgen`, `wasm-bindgen-test` and `zip` declared once in the workspace).
- Common traits: ok (`Plan`: `Debug`, `Clone`, `PartialEq`, `Serialize`; `File`, `Choices`: also `Eq`).
- Conversions through `From`: ok (`File` from Isoloom's `GeneratedFile` and the compiler's `LabFile`; `Error` from the compiler's and zip's).
- Types carry meaning, `#[non_exhaustive]`: ok (`Error`; `Plan`, `File` and `Choices` are the editor's JSON).
- Ownership: ok.
- `#[must_use]`: ok.
- Visibility: ok.
- Library errors with `thiserror` and a `Result` alias: ok.
- Error messages: ok.
- No panics in library code: ok (JSON that can't be written falls back to a minimal answer, never a panic across the WASM boundary).
- Sync or async, observability: ok (synchronous; logs nothing).
- Crate docs with a runnable example, `# Errors` and `# Panics`: ok.
- Docs build clean: ok.
- Behaviour-named tests, one concept each: ok.
- Property tests for parsers: ok (the topology parser is the compiler's, property-tested and fuzzed there).
- Snapshots only for whole files: ok (none; precise assertions).
- Mutation testing: not in scope (the compiler is mutated; this crate is glue around it and Isoloom).
- Fuzzing: ok through the compiler's `drawing` target (the same parser and compiler).

## Found on the way
- A one-target download dropped the controller's playbooks (Isoloom runs them from their folder, naming only the file): a lab file now counts as used when the target's files name it by path or file name (`bundle::uses`, tested).

## 1c organization
- `lib.rs` holds only the module tree and the re-exported API: ok (the `#[wasm_bindgen]` functions moved to `js`).
- Modules by domain concept: ok.
- One `Error` in `error.rs` with a `Result` alias: ok.
- Tests next to the code, integration tests in `tests/`, fixtures in `tests/fixtures/`: ok.
