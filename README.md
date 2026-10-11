# Isothrum

The engine behind [isothrum.com](https://www.isothrum.com): draw a network lab (machines and
the cables between their ports) and get an [Isoloom](https://github.com/isoloom/isoloom) spec
and every file Isoloom generates from it.

- `crates/isothrum-core`: the lab model and its compiler. A LAN becomes a network whose machines
  take its free addresses; a switch's VLANs nest under a LAN named after the switch; each cable
  becomes its own point-to-point network (a /29 cut from `10.200.0.0/16`); an automatic router
  becomes `reach` rules between its networks, and a Cisco router its LANs' gateway. The result
  is an `isoloom.yml` written without Isoloom's defaults.
- `crates/isothrum-wasm`: the same, compiled to WebAssembly for the browser editor: the spec,
  its validation, the generated files, and one zip to download.
- `apps/desktop`: the desktop app (Tauri): run a lab with Docker or Vagrant, open consoles,
  start, stop and snapshot machines, capture traffic. Its own workspace (it needs GTK/WebKit).
- `docs/GNS3_COVERAGE.md`: which GNS3 features Isothrum covers, plans or leaves out.

## Build

Rust 1.88 or later.

```
cargo test
cargo clippy --all-targets -- -D warnings
wasm-pack test --node crates/isothrum-wasm      # the exports, in WebAssembly
wasm-pack build crates/isothrum-wasm --target web --out-dir ../../web/pkg --release
```

CI runs these, the desktop app's tests on Linux, macOS and Windows (with the Docker tests on
Linux), and a check on Rust 1.88.

The website (`isoloom/isothrum-website`) serves `web/pkg/isothrum_wasm.js` and
`isothrum_wasm_bg.wasm` from `public/wasm/`.
