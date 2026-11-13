# Handoff — Image-Generating Guides (dapple-ui)

> **Audience:** dapple-ui team (incl. dapple-ui-demo).
> **Status:** backend ready and **validated against a real run** (see Field
> report below); **dapple-ui needs work** before it can display this class of
> guide. This doc is the gap list.

## What changed

A new class of guide produces an **image** as its deliverable, not just text. The
first one is **Wimmelbook Spread Generator** (`dapple-guides`
`public/dapple/wimmelbook-spread-generator/`): the operator builds a busy
hidden-object scene, the backend renders it, and the journey's final outputs are
a **picture** (the spread), a **find-list** (checklist), and a **solution**.

dapple-ui has only ever consumed text guides (e.g. Shaggy Dog Joke Builder). The
journey/stage/step/dispatch/brief machinery is identical — **most of the UI works
as-is** — but four things are new. Three are real UI work.

## Field report (2026-06-22 — first real run on dapple-ui-demo)

A real journey (`019ef170-a737-78a0-aea4-5e13eda1d220`, dapple namespace) ran the
guide all the way to the final "Name and Render" pane. The render **worked**: the
backend generated and bound a valid **2.63 MB `image/png`** asset
(`019ef175-b650-…`, 11 chunks, bytes match `content_length`, decodes to a clean
1536×1024 PNG). So the pipeline is real. But the final pane showed:

```
Find these     →  Nothing to show.
Your spread     →  Couldn't load the image.   [Open it directly]
Solution        →  Nothing to show.
```

Both symptoms were diagnosed end-to-end. Exactly one is UI work:

| Symptom | Real cause | Whose fix |
|---|---|---|
| **"Couldn't load the image"** | The asset bytes are fine and bound. `GET /api/assets/{id}/content` requires auth (`RegisteredUser`); a browser `<img src=…>` can't send the `Authorization` header → 401 → broken image. | **UI** — proxy it (see §1, below). |
| **"Nothing to show"** (find_list + solution) | The lists were genuinely empty `[]`. A guide-side render bug made the render task see **0 shots**, so it produced only the bare background and no findables/solution. Fixed in `dapple-guides`; **pending the guide re-sync**. | **Backend/guide** (done, pending re-sync). The UI was rendering empty arrays correctly. |

Takeaway for the UI: **"Nothing to show" was not a UI bug** — once the guide is
re-synced and a fresh journey is run, those slots arrive non-empty and the UI
just renders the checklist/table. **"Couldn't load the image" is the one item on
your plate**, and it is the headline gap below.

## What's the SAME (no change)

- It is a normal Journey → Stage → Step → Dispatch flow.
- The generate→pick→refine pattern, `select`/`checkbox` inputs, and
  `POST /api/dispatch-intent-bindings` for operator picks are unchanged.
- Computed outputs are produced and read the same way the assembled-story output
  is in Shaggy Dog. The opening stage's **style** and **difficulty** are ordinary
  `select`s over computed candidates — nothing new.

## What's NEW (the gaps to close)

### 1. Asset-kind outputs — render an image, not a UUID  ⟵ headline
One exposed slot, `wimmelbook_puzzle_image`, has **`value_kind: "asset"`**
(`producer: "computed"`). When bound, its value is an **asset UUID**, not text.

The UI already resolves bound computed values for display (Shaggy Dog
interpolates the computed story into a `:::brief-markdown`). The new requirement:
when a resolved value's `value_kind` is `asset`, **render an image from the
asset content endpoint** instead of printing the id:

- `GET /api/assets/{id}` — metadata (mime type, length).
- `GET /api/assets/{id}/content` — the raw bytes. Returns `200` with
  `Content-Type: <asset mime>` (`image/png` here) + `Accept-Ranges: bytes`;
  honors HTTP `Range` with `206` for `<video>`/`<audio>` later. **This endpoint
  is authenticated.**

#### ⚠️ The content endpoint is auth-gated — proxy it; do NOT point a bare `<img src>` at it

`asset_content_handler`
(`crates/dapple-prompt-api/src/handler/assets.rs:402`) takes a `RegisteredUser`
extractor — every request needs the internal API's auth (the service token /
JWT). A browser `<img src="…/api/assets/{id}/content">` issues an
**unauthenticated** GET (an `<img>` tag cannot attach an `Authorization`
header), so the API rejects it before the handler runs → the broken-image state
seen in the field report. `fetch()` with a bearer token "works" but then you're
holding bytes you still have to turn into an object URL, and you've leaked the
token to the browser.

**Do what the UI already does for every other API call: proxy it server-side.**
dapple-ui/-demo already calls the internal API from its own server with the
service token and never exposes that token to the browser. Add an equivalent
**same-origin, server-side image route** that streams the asset:

- e.g. `GET /img/asset/{id}` (or a Next route handler) on the UI server →
  server-side `fetch` of `…/api/assets/{id}/content` **with the service token** →
  pipe the body back with the upstream `Content-Type` (and `Content-Length` /
  `Accept-Ranges` if you want range support).
- The browser then renders `<img src="/img/asset/{id}">` — same-origin,
  cookie/session-authed, no token in the page.
- Pass through `Range`/`206` only if you later show streamed media; for the PNG a
  plain full-body proxy is enough.

This proxy is the **one genuinely new piece of plumbing**. Rendering the `<img>`
itself is trivial once the bytes are reachable.

### 2. JSON outputs the operator should actually read
Three computed outputs are `value_kind: "json"` and are meant to be shown:

- `wimmelbook_find_list` — an array of strings: the "find these things"
  checklist. Render as a list (ideally tickable next to the image).
- `wimmelbook_solution` — an array of `{ item, region:{x,y,w,h} }`: where each
  findable was placed. **Spoiler** — put it behind a reveal/toggle.
- `wimmelbook_shotlist` — the Art Director's plan (see #3).

Raw-JSON-dumping these (e.g. via naive markdown interpolation) is not acceptable
UX. The UI should read the slot values and render them structured.

> Authoring note we may revisit together: the guide's final
> `:::brief-markdown` currently interpolates `$wimmelbook_shotlist` directly,
> which would dump JSON. The UI should prefer rendering the structured slots
> (`find_list`, `puzzle_image`) over the interpolated markdown; we can change the
> guide's deliverable block to match whatever the UI wants to consume.

### 3. A review-before-render gate ("Board the Shots")
There is a dedicated stage where a computed **`shotlist`** (json) is produced for
the operator to **review and approve before the expensive render**. There is no
explicit "approve" button in the data model — **approval = the operator advancing
and dispatching the next (render) stage.** If the shotlist isn't right, the
operator **re-runs the stage** (re-dispatch = refine) for a fresh take.

UI work: display the shotlist readably (it's `{ base_prompt, shots:[{ render_prompt,
findables:[{name,spot}] }] }`), and make "looks good → render" vs "redo" obvious.
This is the writer→artist handoff made visible; treat it as a real review screen,
not a throwaway computed output.

### 4. Long-running render dispatch
The render task makes **1 + N + M image-generation calls** (a background render,
one `imagine` per shot block, one per findable) and then a **single deterministic
`composite`** that assembles the pieces locally (no model touches the composed
pixels). The generation calls dominate and **this can take minutes**, not the
sub-second of a text step.

UI work:
- The dispatch poll loop must **tolerate long latency** and show a clear
  "rendering your spread, this takes a bit" state. Do not time out early.
- Progress signal: `GET /api/dispatches/{id}/task_attempts` carries `signals`
  and (on the producing step) `computed_outputs`, which can drive a progress
  hint. Phase stays non-terminal (`running`) until done.

## API touchpoints (all existing endpoints)

| Endpoint | Use | New for this guide? |
|---|---|---|
| `GET /api/journeys/{id}/steps` | exposed-slot status; each slot has `value_kind`, `producer`, `bound`, `input?` | **value_kind can be `asset`** |
| `GET /api/journeys/{id}/stages` | per-pane completion | no |
| `POST /api/dispatch-intent-bindings` | operator picks | no |
| `POST /api/dispatches`, `GET /api/dispatches/{id}` | fire + poll | **render is slow** |
| `GET /api/dispatches/{id}/task_attempts` | `signals` / `computed_outputs` progress | helpful for the render wait |
| `GET /api/assets/{id}` + `/content` | image metadata + bytes | **new: `/content` is auth-gated — proxy server-side, render image** |

## Suggested UI checklist

- [ ] Add a **server-side authenticated image proxy** (e.g. `/img/asset/{id}`)
      that streams `/api/assets/{id}/content` with the service token; point the
      `<img>` at the proxy. **Do not** put `/api/assets/.../content` in a browser
      `<img src>` — it 401s. (This is the field-report headline.)
- [ ] Detect `value_kind: "asset"` on a bound computed slot and render the image
      from the proxy (with a loading/fallback state).
- [ ] Render `find_list` as a checklist; render `solution` behind a reveal.
- [ ] Build the "Board the Shots" review screen (show shotlist, approve = advance
      / redo = re-dispatch).
- [ ] Make the render dispatch wait long-latency-friendly (progress state, no
      early timeout); optionally surface `task_attempts.signals`.
- [ ] Decide the deliverable layout (spread image + checklist; solution toggle).
      "Play the puzzle" (click-to-find) is a nice future, not required for v1.

## Dependencies / rollout

This guide is inert until **both** are true:
1. **conduit is deployed with the image backend** (this is built and merged in
   `dapple-prompt`; it needs `OPENAI_API_KEY` set on the conduit deployment), and
2. the **guide is ingested**.

Until then the guide either won't appear or its render step will fail with an
"image service not configured" error. There is nothing for the UI to test
against before the conduit is deployed with a key.

## One-line summary for the standup

> A new guide outputs a rendered image plus a find-list. The flow is the normal
> journey/dispatch one, but the UI now has to (a) render an `asset`-kind output as
> an image — via a **server-side proxy** of `/api/assets/{id}/content` (the
> endpoint is auth-gated; a bare `<img src>` 401s), (b) show JSON outputs
> (checklist / shotlist) nicely, (c) add a shot-list review step, and (d) tolerate
> a minutes-long render dispatch. First real run confirmed the backend produces
> the image correctly; the only UI-side blocker observed was the missing image
> proxy.
