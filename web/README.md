# dbota in a browser

A browser build plays a replay built into it, drawn through WebGPU (or WebGL2), with the asset
cache served next to the page. It needs `wasm32-unknown-unknown` and `wasm-bindgen-cli` at the
version in `Cargo.lock`; `bota_client_core` builds for `wasm32` (its live socket is
desktop-only, so a page plays the built-in replay).

```sh
DBOTA_WEB_REPLAY=$PWD/match.brp \
DBOTA_WEB_ARGS="--replay built-in --follow 0" \
    cargo build --profile web --target wasm32-unknown-unknown --features web-replay
wasm-bindgen --target web --no-typescript --out-dir web/out \
    target/wasm32-unknown-unknown/web/dbota.wasm
cp web/index.html web/out/
ln -s "$PWD/assets" web/out/assets
ln -s ~/.cache/dbota/v<N> web/out/cache        # the page fetches the cache as cache/...
python3 -m http.server --bind 127.0.0.1 --directory web/out 8080
```

`DBOTA_WEB_ARGS` are the command line the page lacks (any of `dbota --help`). Keep the replay
short: it is part of the program's memory. With `?report` in the page's address, the console
(the client logs its memory every five seconds) goes to the server's log as `/report?...`
requests. Valve's files stay in the local cache: serve them only to yourself.
