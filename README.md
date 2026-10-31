# playwright-rs

Async Rust bindings for [Microsoft Playwright](https://playwright.dev) browser
automation.

> 使用文档（中文）：[docs/USAGE.md](docs/USAGE.md)

Like the official Python, Java and .NET bindings, this crate does **not**
reimplement Playwright. It launches the same Node.js *driver* as a child process
and controls it over Playwright's length-prefixed JSON protocol. That means the
behaviour matches upstream Playwright exactly; this crate only provides the
typed, `async` Rust surface over tokio.

## Two layers

**1. The complete generated protocol layer (`playwright::protocol`).**
Every interface, command, event and type in the Playwright protocol is ported.
It is generated directly from Playwright's own protocol spec (the `spec/*.yml`
files, pinned to the driver version) by the `codegen` crate, producing a typed
channel wrapper per interface with an `async` method per command, typed
parameter/result structs, typed initializer structs, and a typed event enum.
Current coverage: **34 interfaces and 677 types** — the full protocol surface
(BrowserType, Browser, BrowserContext, Page, Frame, ElementHandle, JSHandle,
Request/Response/Route, WebSocket, Worker, Dialog, Download, Tracing, CDPSession,
APIRequestContext, Android, Electron, and more).

**2. The ergonomic layer (`playwright::api`).**
A typed, `snake_case` wrapper for **every** interface, also generated from the
spec: channel-returning commands hand back the typed wrapper object (not a raw
guid), single-field results are unwrapped (`title()` returns `String`,
`wait_for_selector()` returns `Option<ElementHandle>`), and every object with
events exposes a typed stream via `events()`. On top of that, hand-written
conveniences add the Python-style niceties: the `Playwright` entry point,
`page.goto(url)` / `page.click(sel)` / `page.evaluate(js)` delegating to the
main frame, `Browser::new_page()`, and `Response::text()`. From any wrapper you
can reach the low-level channel with `.channel()` or the connection with
`.connection()`.

**Client-side abstractions.** On top of both layers, the crate provides the
concepts that have no protocol object of their own:

- **`Locator`** — a lazy `(frame, selector)` handle: `page.locator("button")`,
  with `.nth(i)`, `.first()`, `.last()`, `.all()`, chaining via `.locator(sub)`;
  actions (`click`, `dblclick`, `hover`, `fill`, `clear`, `type_text`, `press`,
  `check`/`uncheck`, `focus`, `tap`, `select_option`, `set_input_files`,
  `drag_to`, `dispatch_event`, `scroll_into_view_if_needed`); and queries
  (`count`, `text_content`, `inner_text`, `inner_html`, `input_value`,
  `get_attribute`, `is_visible`/`hidden`/`enabled`/`disabled`/`checked`/`editable`,
  `bounding_box`, `aria_snapshot`, `all_text_contents`, `all_inner_texts`,
  `element_handle`, `screenshot_png`, `evaluate`).
- **Assertions** — auto-retrying web-first assertions that poll until the
  condition holds or a timeout elapses (`.not()` and `.timeout(..)` on each):
  - `expect(&locator)`: `to_be_visible`/`hidden`/`attached`/`enabled`/`disabled`/
    `editable`/`focused`/`checked`, `to_have_text`, `to_contain_text`,
    `to_have_value`, `to_have_attribute`, `to_have_count`.
  - `expect_page(&page)`: `to_have_title`, `to_have_url`.
  - `expect_response(&api_response)`: `to_be_ok`.
- **Typed event streams** — each returns a typed
  `tokio::sync::mpsc::UnboundedReceiver`; gated events (console/dialog/request/
  response/…) are enabled automatically:
  - `context`: `on_console`, `on_dialog`, `on_dialog_closed`, `on_request`,
    `on_request_finished`, `on_request_failed`, `on_response`, `on_page`.
  - `page`: `on_close`, `on_crash`, `on_worker`, `on_download`,
    `on_file_chooser`, `on_frame_attached`, `on_frame_detached`.
  - `Download`: `save_as`, `path`, `failure`, `delete`, `cancel`.
  - `FileChooser`: `set_files`, `element`, `is_multiple`.
- **Input devices** — `page.keyboard()` (`down`, `up`, `press`, `type_text`,
  `insert_text`), `page.mouse()` (`move_to`, `down`, `up`, `click`, `dblclick`,
  `wheel`), `page.touchscreen()` (`tap`).
- **Network interception** — `page.route(glob, handler)` /
  `context.route(glob, handler)` with an async handler receiving a `Route`
  (`continue_`, `fulfill_text`, `abort_now`, `request`, `url`). A single
  dispatcher fans events to handlers and auto-continues unmatched requests.
  `page.unroute(glob)` / `context.unroute(glob)` remove routes.

**More Python-parity helpers.**

- **`get_by_*` locators** on `Page`/`Frame`/`Locator`: `get_by_text`,
  `get_by_role(role, name)`, `get_by_label`, `get_by_placeholder`,
  `get_by_alt_text`, `get_by_title`, `get_by_test_id`.
- **`frame_locator(selector)`** → `FrameLocator` for drilling into iframes.
- **Tracing** — `context.tracing().start(name, screenshots, snapshots)`,
  `start_chunk`, `stop(path)` (saves a trace zip).
- **Clock** — `context.clock()`: `install`, `install_at`, `set_fixed_time`,
  `set_system_time`, `fast_forward`, `run_for`, `pause_at`, `resume`.
- **WebSocket** — `ws.on_frame_sent()`, `on_frame_received()`, `on_close()`,
  `url()` (frames delivered as `WebSocketFrame`).
- **Dialog** — `dialog.message()`, `dialog_type()`, `accept_text(prompt)`.
- **Request/Response accessors** — `request.url()`, `method()`,
  `resource_type()`, `is_navigation_request()`, `headers()`;
  `response.headers()`, `json()` (plus the existing `status`/`ok`/`text`).

**Typed multi-field returns.** Commands whose result has several fields return a
generated struct with channel fields already wrapped, e.g.
`BrowserType::launch_persistent_context` returns
`{ browser: Browser, context: BrowserContext }` rather than raw guids.

```rust,no_run
# use playwright::{expect, Playwright};
# async fn f(page: playwright::Page) -> playwright::Result<()> {
let submit = page.locator("button[type=submit]").await?;
submit.click().await?;
expect(&page.locator("#status").await?).to_have_text("Saved").await?;
# Ok(()) }
```

### Regenerating the protocol layer

Both generated files — `src/protocol/generated.rs` (channel layer) and
`src/api/generated.rs` (ergonomic layer) — are checked in. To regenerate them
(e.g. after updating the specs in `spec/` to a new Playwright version):

```sh
cargo run -p codegen
```

## Prerequisites

You need Node.js plus the Playwright driver and browser binaries. During
development the simplest setup is a `driver/` folder in the project root:

```sh
mkdir driver && cd driver
npm init -y
npm install playwright-core
node node_modules/playwright-core/cli.js install chromium
node node_modules/playwright-core/cli.js install chromium-headless-shell
```

The driver's `cli.js` is located automatically (see `Driver::locate`). Override
with environment variables if needed:

- `PLAYWRIGHT_CLI_JS` — full path to the driver `cli.js`.
- `PLAYWRIGHT_DRIVER_DIR` — a folder containing `node_modules/playwright[-core]`.
- `PLAYWRIGHT_NODE` — the `node` executable to use (defaults to `node` on `PATH`).

> Note: Playwright's default `headless: true` launch uses the separate
> `chrome-headless-shell` binary, so install both `chromium` and
> `chromium-headless-shell` as shown above.

## Example

```rust,no_run
use playwright::Playwright;

#[tokio::main]
async fn main() -> playwright::Result<()> {
    let mut pw = Playwright::initialize().await?;
    let chromium = pw.chromium().await?;
    let browser = chromium.launch().await?;

    let page = browser.new_page().await?;
    page.goto("https://example.com").await?;
    println!("title: {}", page.title().await?);

    let shot = page.screenshot().await?;
    std::fs::write("example.png", &shot)?;

    browser.close().await?;
    pw.close().await?;
    Ok(())
}
```

Options that Playwright accepts as objects are passed as `serde_json::Value`
through the `*_with` methods, e.g.:

```rust,no_run
# use playwright::Playwright;
# async fn f(chromium: playwright::BrowserType) -> playwright::Result<()> {
let browser = chromium
    .launch_with(serde_json::json!({ "headless": false, "args": ["--start-maximized"] }))
    .await?;
# let _ = browser; Ok(()) }
```

## Runnable examples

```sh
cargo run --example handshake   # launches the driver, prints engine names (no browser binaries needed)
cargo run --example offline     # ergonomic layer: full DOM/scripting/screenshot flow, no network
cargo run --example generated   # generated protocol layer: launch + setContent + title via typed channels
cargo run --example locator     # Locator, expect assertions and typed console events, no network
cargo run --example basic       # navigates to example.com (needs network)
```

## Tests

```sh
cargo test
```

Unit tests cover the `SerializedValue` codec. The offline integration test
(`tests/integration.rs`) drives a real Chromium headless via `set_content` and
exercises locators, assertions, page assertions and typed console events; it
skips automatically if the driver isn't installed.

## Architecture

```
Playwright (owns the driver process)
  └─ Connection  ── transport (4-byte LE length prefix + JSON over the driver's stdio)
       ├─ request/response correlation by id
       ├─ remote-object registry (built from __create__ / __dispose__ events)
       └─ per-object event subscriptions
```

- `transport` — frame reading/writing.
- `connection` — the dispatcher and object registry.
- `driver` — locating and spawning the Node driver.
- `serialized_value` — `serde_json::Value` ⇄ Playwright `SerializedValue`.
- `protocol` — the **generated** complete channel layer (`src/protocol/generated.rs`).
- `api` — the **generated** ergonomic layer (`src/api/generated.rs`) plus
  hand-written conveniences (`src/api/ext.rs`).
- `codegen/` — the generator; `spec/*.yml` — the pinned protocol spec it reads.

## License

Licensed under the MIT License. See [LICENSE](LICENSE).
