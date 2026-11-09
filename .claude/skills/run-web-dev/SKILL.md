---
name: run-web-dev
description: Run the Yomeru website and yomeru-server locally in debug/dev mode (server on 127.0.0.1:4500, Dioxus dev server on 127.0.0.1:8080 proxying /api to it). Trigger when the user asks to run, start or launch the servers, the website, the web app, or to test a change in the web app.
---

# Run the website + server in dev mode

Two processes, both started with Bash `run_in_background: true` (they never exit).

## 1. API server (port 4500)

`app/web/Dioxus.toml` proxies `/api/*` to `http://127.0.0.1:4500/api/`, so the
server **must** be on 4500. `--dev-mode` binds to loopback and logs in any email
without an OTP (no SMTP needed). Use the dev DB, never `yomeru.db`.

```bash
cd /home/guillaumek/Git/japanese/Yomeru && cargo build -p server && \
  exec ./target/debug/yomeru-server --dev-mode --port 4500 \
    --db yomeru-web-dev.db --data-dir extension/data
```

`extension/data/` must hold `jmdict.bin`, `kanjidic.bin`, `examples.bin`
(build with `cargo xtask build-all` if missing). Spawn the binary, not
`cargo run`, so stopping the task actually stops the server.

## 2. Web dev server (port 8080)

**Run from the repo root with `-p`**, not from `app/web`: dioxus-cli 0.7.10
canonicalizes the workspace `default-members` paths against the current
directory and panics (`workspace.rs:325 … NotFound`) when started in `app/web`.
`--interactive false` because there is no TTY.

```bash
cd /home/guillaumek/Git/japanese/Yomeru && exec dx serve -p yomeru-web --interactive false
```

The first build takes ~20 s; it's ready when its output shows
`Build completed successfully`. It hot-reloads on file changes.

## 3. Wait for both, then smoke test

Wait with one background Bash that exits when both ports listen:

```bash
until ss -ltn | grep -q '127.0.0.1:4500 ' && ss -ltn | grep -q '127.0.0.1:8080 '; do sleep 1; done
```

Then confirm the page and the proxied API:

```bash
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8080/
curl -s -X POST -H 'content-type: application/json' \
  -d '{"text":"ちぢ","max":5}' http://127.0.0.1:8080/api/lookup-prefix | head -c 300
```

Tell the user to open http://127.0.0.1:8080/. To stop, TaskStop both tasks.

If a port is already taken, check `ss -ltnp | grep -E ':(4500|8080)'`: an
earlier run is probably still up and can be reused.
