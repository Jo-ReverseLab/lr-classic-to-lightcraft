# LightCraft control port

> **Status:** Implemented in `crates/lc-writer/src/control.rs`: the client, the pre-flight checks and handling of the save-failure reply. `ui.inspect` is sent as a port method, not through `engine.execute`. LightCraft must be started with `--control <port>`. Items marked as assumptions were not verified against a running app.

All paths are relative to the LightCraft source root (the LightCraft source checkout, read only). Anything not backed by a file:line is marked *(assumption)*.

## 1. The control port

- **Start**: `lightcraft --control <port>` or env `LIGHTCRAFT_CONTROL_PORT` (`apps/lightcraft/src/main.rs:3`, `:817`, `:898`, `:906`). A non-numeric value is logged and ignored, no server (`main.rs:868-875`). The docs example uses 7980, but there is no fixed default: without the flag no port is open (`docs/control-protocol.md:3`).
- **Bind**: `TcpListener::bind(("127.0.0.1", port))`, loopback only (`apps/lightcraft/src/control_server.rs:28`). Bind failure is only logged (`:31`), so the writer must verify the connection works. At most 16 connections (`control_server.rs:24`, `:55`). No authentication (`docs/control-protocol.md:41`).
- **Protocol**: JSON lines, one request per line, one reply per line, in order (`docs/control-protocol.md:4-10`). Each line must be a JSON object with a string `method`; `id` and `params` optional (`:34`). Anything else (not JSON, no `method`, invalid UTF-8, line over 4 MiB) gets one error reply and **the server closes the connection** (`:34-37`).
- **Method names**: the implementation accepts `"engine.execute" | "ui.menu.invoke" | "command"` (`crates/ui-egui/src/control.rs:329`). So the request shape `{"method":"command","params":{"command":…,"params":…}}` is valid. The documented canonical name is `engine.execute` (`docs/control-protocol.md:7`, `:78`); prefer it. The command name is read from `params.command` (or `params.id`), command parameters from `params.params`, default `{}` (`control.rs:330-331`).
- **Reply**: success `{"id":…,"ok":true,"result":<value>}`; error `{"id":…,"ok":false,"error":"<text>"}` (`docs/control-protocol.md:8-9`; `control.rs:75-79`). The `id` is echoed (doc example; echo in the transport not verified *(assumption)*). Errors are plain strings, no error codes.
- **Timing**: requests are answered on the UI thread between frames, timeout 60 s. If the app has not started the request and draws no frame for 5 s, the reply is `ok:false` and the request is **never run later**, so a retry does not run it twice (`docs/control-protocol.md:12-19`). On Wayland a minimised/hidden window draws no frames (`:16-18`). A started request still gets the full 60 s (`:19`). Idle connections close after 5 minutes (`:22-25`). If the connection drops after sending, the outcome is unknown and the client **must not blindly replay** (`:30-32`). Exception (LightCraft 0.21.1, `docs/control-protocol.md` "idle read timeout"): an idle close sends one line without `id`, `{"ok": false, "error": "control connection idle read timed out; closing the connection", "closed": "idle"}`; older apps send it without `closed`. A request that crossed this notice never ran and may be sent once more on a new connection. Implemented in `crates/lc-writer/src/control.rs` (`is_idle_notice`): resent once, a second notice is `ConnectionLost`.
- **Save failure**: when the journal write fails the command replies `ok:false` with `saved in memory but not written to disk: …`, yet the change **is applied** and retried later (`docs/control-protocol.md:158-168`). The writer must treat this specific error as "applied", not as failed.
- **Background tasks**: 
  - Lightroom import: `library.importLightroom {path}` starts a task; `{status:true}` (or no `path`) reports status, `{status:true, wait:true}` blocks up to 600 s, `{status:true, cancel:true}` cancels (`crates/ui-egui/src/lightroom_import.rs:405-416`). Not relevant to lr2lc itself.
  - General: `activity.list` returns `{tasks:[{id,kind,label,done,total,unit,detail,cancellable,cancelling,ageMs}]}`; `activity.cancel {id}|{all:true}` (`docs/control-protocol.md:99-100`). `ui.inspect` has `import: {done,total,imported,cancelled}|null` ("an import runs on a worker thread") and `tasks`, `activity` (`:82`).
  - **Open**: whether `library.import` itself runs synchronously in the control call or in the background (the `ui.inspect import` field suggests the UI path is a worker; the engine command body at `crates/engine/src/cmd/library.rs:1634-1660` runs `import_with` inline). Through `engine.execute` it appears synchronous, subject to the 60 s timeout; batch accordingly *(assumption, not run)*. The user's phrase `{"status":true}` matches only `importLightroom`.
- **One library at a time**: a library is open in one program at a time via `catalog.lock`; a second opener fails with "This library is already open in LightCraft (process N …)" and the doc says to use connect mode against the running app (`docs/mcp.md:19-22`). The control port therefore always acts on the single library the app has open. `app.openLibrary {path}` switches it (`docs/control-protocol.md:197`; refuses Lightroom catalogs, `:216-224`).

## 2. Which library is open

- `library.source` does **not** answer this. It selects what the browser shows (`kind: all|recentlyAdded|album|recentlyDeleted|picks|missing|libraryFolder`, `crates/engine/src/cmd/library.rs:383-389`).
- **`library.info {}`** (`library.rs:1662`): returns `{"persistent":false,…}` for an in-memory/temporary session, else `{"persistent":true,"path":<library dir>,"photos":n,"albums":n,"seq",…,"unsavedOps",…}` (`library.rs:1676-1690`). `path` is the library folder. Use this to compare against the target library and to refuse a temporary session (`persistent:false`).
- `app.recentLibraries` returns `[{path,name,exists,checking,open,lastOpened}]` with an `open` flag (`docs/control-protocol.md:200-203`).
- `ui.inspect` is a method of the control port itself, **not** an engine command: it is sent as `{"id":N,"method":"ui.inspect","params":{}}`, never through `engine.execute` (which rejects it as an unknown command). Its `libraryProblem` is non-null when the app has no usable library (`docs/control-protocol.md:170-177`); the writer should abort in that case.

## 3. XMP auto-write

- A queryable setting exists: **`library.xmpPreferences`** with params `{}` returns the current preferences `{autoWrite, naming}`; with `{autoWrite?: bool, naming?: stem|full}` it sets and saves them with the library (`crates/engine/src/cmd/xmp.rs:65-80`, `:101-108`). With an empty object nothing is saved, it only returns `s.xmp` (`xmp.rs:73-75`). Reading is therefore side-effect free.
- Reply field names are camelCase (`crates/engine/src/sidecar.rs:56-61`: `#[serde(rename_all="camelCase")]`, fields `auto_write`, `naming`). Result: `{"autoWrite":bool,"naming":"stem"|"full"}` *(naming spelling inferred from the doc string)*.
- Default is `false` (`#[derive(Default)]`, `sidecar.rs:56-61`). It is persisted per library ("saved with the library", `xmp.rs:107`; `s.save_prefs()` `xmp.rs:73`). **So an existing library may have it on: always query it**.
- Effect when on: after every successful command that adds to the pending log, sidecars are written next to the originals (`crates/engine/src/lib.rs:603-606`). This also applies to commands run through the control port (the check is in the generic execute path *(assumption: `lib.rs:603` is that path; not fully traced)*), and import under `auto_write` writes sidecars too (`crates/engine/src/import.rs:1537-1561`). This would violate principle 2 ("no XMP sidecars").
- Also `library.toggleAutoWriteXmp {}` → `{autoWrite}` toggles (`xmp.rs:123-126`): do not use it.
- **Recommendation for the writer**: query `library.xmpPreferences {}` before any write; if `autoWrite` is true, abort with a clear message (changing the user's preference silently is not allowed by the principles; ask the user to turn it off, or add an explicit opt-in flag). Never call `photo.saveMetadataToFile`.

## 4. `library.import`

Parameters (`crates/engine/src/cmd/library.rs:1638`): `paths` (files or folders, recursive), `mode?: add|copy|move`, `destination?`, `organize?`, `rename?`, `renameStart?`, `metadataPreset?`, `dng?`, `local?`, `album?: albumId`, `albumName?`, `preset?: presetId`, `keywords?: [..]` (applied to every imported file), `onDeleted?: restore|skip|fresh`. Result: `{imported, duplicates:[{path,existing,reason,existingDeleted?}], restored?, failed, warnings?, moved?, kept?, album?}` (`library.rs:1638`).

- **Reference only**: `mode: "add"` = "reference the files in place" (`library.rs:1638`; `crates/engine/src/import.rs:47-49`; `docs/lightroom-catalog-import.md:59`). `copy` writes into the library's `Originals/…`, `move` also deletes the sources (`library.rs:1638`, `import.rs:51-55`). The mode parser defaults to `add` when `mode` is absent (`library.rs:53` `unwrap_or("add")`; `import.rs:47-49` `#[default] Add`) and accepts only `add|copy|move`, erroring otherwise (`library.rs:53-58`).
- Still send `"mode":"add"` explicitly (defensive), and never send `copy`/`move`/`dng`/`organize`/`rename`/`destination`.
- Other side effects to avoid: `local: true` keeps photos out of the library (`library.rs:1638`) — do not send. `metadataPreset`/`preset` change settings — do not send. `keywords` applies one list to all files — do not use for per-photo keywords.
- Existing XMP sidecars are read at import and win over catalog data (`import.rs:15-17`; `docs/xmp-interop.md:13`). Import itself reads them only; with auto-write on it can write (see 3).
- "Guaranteed" non-modification of originals is **not proven by a full code audit** (inherited limitation from `lightcraft-import.md:56`); it follows from the documented mode semantics. Recommendation: lr2lc's own checks (hash of originals before/after, if wanted by the user) as belt and braces.
- Idempotency: duplicates by path and content hash are skipped and reported with `existing` photo id and `reason` (`import.rs:1-24`, `:638`; result `duplicates` at `library.rs:1638`). A second run with the same paths returns them in `duplicates` rather than creating new photos.

## 5. Per-photo commands

All take `ids?` (array of photo ids). **Always pass `ids`**: without it the command works on the UI selection and may move the selection (`library.rs:799-803`, `:832-837`, `:907-912`).

| Need | Command | Parameters | Idempotent? |
|---|---|---|---|
| Stars | `photo.rate` (`library.rs:799`) | `{rating: 0..5, ids?, advance?}`; other values error (`library.rs:806-809`) | Yes, sets an absolute value *(assumption from semantics)* |
| Flag | `photo.flag` (`library.rs:832`) | `{flag: pick\|reject\|none, ids?, advance?}` (`:839`) | Yes, absolute |
| Colour label | `photo.label` (`library.rs:907`) | `{label: red\|yellow\|green\|blue\|purple\|none, ids?, advance?}`; missing/non-text label is an error (`:918-920`) | Yes, absolute. Only these five labels; Lightroom's other label names do not exist (report as unmapped) |
| Title, caption | `photo.setMeta` (`library.rs:1033-1037`) | `{ids?, title?, caption?, altText?, extendedDescription?, copyright?, creator?, location?, city?, state?, country?, gps?, keywords?, addKeywords?, removeKeywords?, …}` | Yes for title/caption/`keywords` (set); `addKeywords` presumably set-union *(assumption, previous doc `lightcraft-import.md:29`: handling at `library.rs:1106-1118` — not re-read)* |
| Keywords with hierarchy | `photo.setMeta {keywords:[…]}` | Hierarchy as `a\|b\|c` strings (`crates/engine/src/cmd/keywords.rs:313`, `lightcraft-import.md:28`) | as above |
| Keyword attributes | `keyword.create` (`keywords.rs:355-360`) | `{name, parent?: keyword\|null, synonyms?, includeOnExport?, exportContaining?, exportSynonyms?, person?, addToSelected?, ids?}` → `{keyword}`; reads/edits via `keyword.info {keyword}` → `{path,listed,count,synonyms,includeOnExport,exportContaining,exportSynonyms,person}` (`keywords.rs:339`); tree via `keyword.list {}` (`keywords.rs:313`) | **Unknown** whether a second `keyword.create` of an existing keyword errors or updates *(open)*. Plan: check `keyword.list` first. Parameter parsing at `keywords.rs:47-54` shows attributes are merged with the existing record (`base.synonyms`), which suggests an update, not an error *(assumption)* |
| Virtual copy | `photo.virtualCopy` (`crates/engine/src/cmd/organize.rs:107`) | `{ids?, name?}` → `{ids:[new ids]}`; new entry shares the file, independent settings, stacked with the original, named `name` or "Copy N" (`organize.rs:110`) | **No**: every call makes a new copy. The writer must keep its own mapping (Lightroom virtual-copy id → LightCraft id) and call it only once per copy |

Note: `photo.virtualCopy` copies the settings of the source photo; a copy created before the master's edits are written will inherit them. Order of operations matters (Open for the develop phase).

## 6. Finding a photo by its path

- A photo's source is `Source::File{path}` (`crates/catalog/src/model.rs:89-94`); `photo.inspect {id}` returns the photo including its source (`crates/engine/src/cmd/query.rs:161-164`: serialises the whole `Photo`, so the path is included *(assumption on the field name, not shown in the lines read)*).
- Best mapping route: **`library.import` result**. `duplicates` entries carry `{path, existing, reason}` (`library.rs:1638`), so for files already known you get path → existing id directly, and for files just imported you have `imported` (ids). Whether `imported` elements are plain ids or `{path,id}` objects is **not verified** (`lightcraft-import.md:14` says "photoId…"); the doc string lists `imported` without shape. Open: check `crates/engine/src/import.rs` report construction. To get a path → id map robustly, import **one file per call**, or import then query.
- Query alternative: `library.filter` takes a `Filter` with `text` (free text over filename, title, caption, keywords…, `crates/catalog/src/query.rs:22-24`) and `libraryFolder` (`query.rs:46-49`). These match by file name or folder, not by exact full path, and change what the UI shows. No dedicated "find by path" command was found in `crates/engine/src/cmd/*.rs` (grep for `find`/`byPath` returned only `library.findSimilar` and `library.findMissing`). Whether a list command (e.g. `library.list`) returns paths: **not checked**.
- Path matching rules: path keys for the Lightroom importer are `\`→`/` and lower-cased only on Windows (`crates/engine/src/lightroom_catalog.rs:36-39`; from `lightcraft-import.md:36`). lr2lc should apply path mapping (principle 6) first and pass absolute paths exactly as it will later compare them.

## Open questions

1. Does `library.import` run synchronously through the control port (60 s limit), or return early? Needs a test against a running app (not possible in this session: no app available, source reading only).
2. Exact shape of `imported` in the `library.import` result.
3. Does `photo.inspect` expose the file path under a stable key, and is there a cheap list command returning `{id, path}` for all photos?
4. `keyword.create` on an existing keyword: error or update? Are synonyms settable after creation (`keyword.info`/an edit command)? Rows at `keywords.rs:385-546` were not read.
5. Is auto-write also applied to commands run via `engine.execute` (`lib.rs:603` generic path)? Likely yes; trace `Session::execute`.
6. Per-command idempotency of `addKeywords` (set union vs. duplicates).
7. Does the `id` of a request get echoed in the reply by `control_server.rs`? The doc shows it; not traced.

## Recommendation

- Use the control port with `engine.execute` (accepted alias `command` also works, `control.rs:329`). One connection, sequential requests, new connection after any error line; never replay after an unknown outcome — instead re-query state (all commands above except `photo.virtualCopy` and probably `keyword.create` are safe to repeat).
- Pre-flight checks, abort on failure: `library.info` (engine command) → `persistent:true` and `path` equals the target library; `ui.inspect` (port method) → `libraryProblem` is null; `library.xmpPreferences {}` → `autoWrite` is false.
- Import only with `mode:"add"` explicitly and only `paths` (+ nothing else); set everything per photo afterwards with `photo.rate`, `photo.flag`, `photo.label`, `photo.setMeta` — always with explicit `ids`.
- Keep lr2lc's own index (Lightroom `id_global` → LightCraft id), because `Photo` has no external-id field (`lightcraft-import.md:44`); virtual copies must be created exactly once.
- Before building the writer, run one live smoke test against a scratch library (`--library`) to close open questions 1–3.
