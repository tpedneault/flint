# Writing a provider

A provider is a program that gives Flint items and, if it wants, carries
out actions on them. Flint starts it (or lets it connect), talks to it in
JSON-RPC over the protocol in [PROTOCOL.md](PROTOCOL.md), and keeps it at
arm's length: a provider that crashes, hangs or sends garbage costs you
its own results and nothing else.

Two complete examples, each under 150 lines and meant to be copied:

| Example | Language | Shows |
|---|---|---|
| [`examples/provider-python/bookmarks.py`](../examples/provider-python/bookmarks.py) | Python, standard library only | a **static** provider: it hands Flint its items once and Flint matches and ranks them |
| [`examples/provider-rust/`](../examples/provider-rust/) | Rust, `serde_json` only | a **dynamic** provider: Flint sends each query as you type and shows what comes back |

Neither depends on anything of Flint's. Both answer a `provider` action
(`execute`), and both are run through Flint's real provider host by the
test suite, so they stay correct.

## Declaring one

Only providers named in `settings.toml` ever run:

```toml
[[providers]]
name = "bookmarks"
command = ["python", "examples/provider-python/bookmarks.py"]
```

- `command` is a list: the program, then its arguments. Flint never runs a
  shell, so there is no quoting to get wrong. The program is looked up on
  the `PATH` as Fenix looks up language servers (on Windows `.cmd` and
  `.bat` shims work), and it starts **in the settings folder**, so a
  relative path is relative to `settings.toml`.
- `connect = true` instead of `command`: Flint starts nothing and waits
  for an already-running program (an app that wants to offer items while
  it runs) to connect to Flint's control endpoint and say `hello` with
  this `name`. Until it does, `flint status` shows it as waiting.
- `prefix = "bm"` overrides the scope keyword the provider asks for.

Docs/CONFIGURATION.md has the rules for each key. Changing `[[providers]]`
takes effect on save: removed providers stop, new ones start, and
unchanged ones keep running.

## The life of a provider

1. **Hello.** The provider speaks first, with a `hello` request naming
   itself (`role: "provider"`, the same `name` as in `settings.toml`).
   Flint answers with its own version. A wrong name or protocol version is
   refused and the connection closed.
2. **Register.** One `provider/register` request: the provider's `id`
   (equal to its name), the `title` shown as its badge and scope chip, a
   `prefix`, whether it is `dynamic`, and its static `items`.
3. **Serve.** From then on:
   - a static provider may send `provider/setItems` whenever its items
     change (Flint re-indexes at most every 250 ms per provider, so a
     burst costs one re-index);
   - a dynamic provider gets a `query` for each change to the query line
     and answers with `results`;
   - either gets `execute` when the user picks one of its `provider`
     actions;
   - either may send `log` lines.
4. **End.** When Flint quits or the provider is removed from the settings,
   Flint closes the provider's stdin. Exit when your input ends.

### Queries and results (dynamic providers)

```json
{"method": "query", "params": {"queryId": 731, "text": "0xff", "scoped": false, "context": {...}}}
{"method": "results", "params": {"queryId": 731, "items": [...], "done": true}}
```

- Every dynamic provider is asked, unless the user scoped the query to a
  different provider. `scoped: true` means the user typed your prefix;
  `text` is then what follows it.
- `minQueryLength` in the registration skips queries shorter than that;
  `debounceMs` (at most 1000) spaces them out: while one waits, a newer
  query replaces it, so you only see what the user settled on.
- Answer in batches of at most 200 items; mark the last `done: true`.
  Flint keeps only results for the **latest** query and only within 2 s
  of sending it. When the user types again you get `cancel` for the old
  query; stop if you can, but answering it anyway is harmless.
- Flint ranks your results with everything else: they are matched against
  the query like any item, and ones that don't match it as text (a
  conversion, a search hit on a description) are kept, after the ones that
  do. Rows already on screen don't jump while results arrive.

`context` says what the user was doing when they summoned Flint:
`foreground` is the window in front (program, title, pid) on Windows; it
is absent on Wayland, which doesn't tell applications about other
windows. `fenix` (the project and file in front in Fenix) arrives with
milestone 7; until then it is absent.

### Items and actions

An item has a **stable** `id` (prefix it with your provider id; frecency
is keyed on it, so the same thing must get the same id across runs), a
`title`, and at least one action, the first being `Enter`'s. PROTOCOL.md
lists every field.

Prefer the **declarative** action kinds: `open`, `reveal`, `copy`, `run`
and `query` are carried out by Flint itself, with no round trip, so they
work instantly and the same way for every provider. Use `kind: "provider"`
only for what only you can do; Flint then sends:

```json
{"id": 9, "method": "execute", "params": {"itemId": "numbers:dec", "actionId": "factor", "query": "360"}}
```

Answer with `{"close": false, "message": "360 = 2 × 2 × 2 × 3 × 3 × 5"}`:
`message` goes in Flint's footer, and `close` (default `true`) says
whether Flint hides. Answer with a JSON-RPC error to report a failure;
its message goes in the footer and Flint stays open. After 10 s without
an answer Flint gives up with "<name> didn't answer".

## When things go wrong

| What happens | What Flint does |
|---|---|
| The program can't be started | The provider is disabled with the reason; `flint status` shows it. |
| It exits, closes its output, or sends something that isn't the protocol | It is restarted after 1 s, then 5 s, then 30 s. After three such stops within a minute it is disabled, with the last reason, until the settings are saved again or Flint restarts. |
| It sends a well-framed message Flint can't use (a malformed or oversized batch, results for an old query, a notification from a newer protocol) | The message is dropped, and a malformed one noted in its log; the provider keeps running. |
| It never answers `execute` | After 10 s the footer says it didn't answer; Flint stays usable meanwhile. |
| It stops while an `execute` is pending | The footer says it stopped before answering. |
| The user types its prefix while it is down | The footer says why nothing answers. |

Nothing a provider does blocks typing or drawing: each provider has its
own threads, and Flint only ever queues work for it.

### Logs

Each provider has its own log, `logs/providers/<name>.log` beside
`flint.log` (see CONFIGURATION.md, "Where things live"): when it started
and stopped and why, what it registered, its `log` messages, ignored
messages, and every line it writes to **stderr**, so print your debugging
there. The log is kept under 1 MiB, with one older copy.

`flint status` lists every provider and its state: `starting`, `running`,
`waiting` (for a `connect = true` one), `restarting` or `disabled`, with
the reason.

## Writing your own

Start from the example in your language, or from either in another: the
framing is a `Content-Length` header, a blank line, and a JSON body, as in
LSP. Then:

- keep item ids stable and prefixed with your provider id;
- write only protocol messages to stdout, and everything else to stderr;
- exit when stdin closes;
- return static items if you can, and let Flint match them: it is faster
  than a round trip per keystroke and ranks them with frecency.
