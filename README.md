# Isothrum

The engine behind [isothrum.com](https://www.isothrum.com): draw a network lab (machines and
the cables between their ports) and get an [Isoloom](https://github.com/isoloom/isoloom) spec
and every file Isoloom generates from it.

- `crates/isothrum-core`: the lab model and its compiler. Each cable becomes its own
  point-to-point network (a /29 cut from `10.200.0.0/16`); the result is an `isoloom.yml`
  written without Isoloom's defaults.
- `crates/isothrum-wasm`: the same, compiled to WebAssembly for the browser editor: the spec,
  its validation, the generated files, and one zip to download.
- `apps/desktop`: the desktop app (Tauri): run a lab with Docker and capture its traffic.
- `docs/GNS3_COVERAGE.md`: which GNS3 features Isothrum covers, plans or leaves out.

## Build

```
cargo test
wasm-pack build crates/isothrum-wasm --target web --out-dir ../../web/pkg --release
```

The website (`isoloom/isothrum-website`) serves `web/pkg/isothrum_wasm.js` and
`isothrum_wasm_bg.wasm` from `public/wasm/`.
