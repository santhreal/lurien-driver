# lurien-driver

The Rust face of **lurien-browser**. Spoken / CLI / MCP: **lurien**.
crates.io name: **lurien-driver**.

```
lurien::Browser::launch(profile)
```

or

```
firefox.launch({ executablePath: "~/.local/share/lurien/lurien" })
```

or

```json
{ "mcpServers": { "playwright": { "command": "lurien-mcp" } } }
```

or

```
lurien serve            # HTTP: many named sessions, one process
```

Every verb is reachable identically from all four: one registry, one
`Session::call`, four transports. `lurien verbs` prints the surface;
`software/browser/docs/VERBS.md` is generated from the same specs.

The engine binary is required. Missing binary is an error. There is no Firefox fallback and no PyPI package.

Install the engine with `software/browser/install.sh`. v1 is Linux x86_64, headful. Captcha claim is managed Cloudflare (`score`) only.

See `software/browser/README.md`.
