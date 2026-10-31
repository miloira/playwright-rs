# playwright-rs 使用文档

Rust 版 Playwright 绑定。它复用 Playwright 官方的 Node 驱动（和 Python/Java/.NET 绑定同一套协议），本身不重新实现浏览器自动化，只提供类型化的异步 Rust API（基于 tokio）。

- 中文使用文档：本文件
- 英文概览：[README.md](../README.md)

---

## 目录

1. [安装与前置条件](#安装与前置条件)
2. [快速开始](#快速开始)
3. [两层 API](#两层-api)
4. [启动与生命周期](#启动与生命周期)
5. [导航与页面内容](#导航与页面内容)
6. [Locator 定位器](#locator-定位器)
7. [expect 断言](#expect-断言)
8. [事件](#事件)
9. [输入设备：键盘 / 鼠标 / 触摸](#输入设备键盘--鼠标--触摸)
10. [网络拦截 route](#网络拦截-route)
11. [Tracing / Clock / WebSocket / Dialog](#tracing--clock--websocket--dialog)
12. [Request / Response](#request--response)
13. [直接使用生成的协议层](#直接使用生成的协议层)
14. [驱动定位与环境变量](#驱动定位与环境变量)
15. [测试](#测试)
16. [重新生成协议层](#重新生成协议层)
17. [错误处理](#错误处理)

---

## 安装与前置条件

需要 Node.js，以及 Playwright 驱动和浏览器二进制。开发期最简单的方式是在项目根放一个 `driver/` 目录：

```sh
mkdir driver && cd driver
npm init -y
npm install playwright-core
node node_modules/playwright-core/cli.js install chromium
node node_modules/playwright-core/cli.js install chromium-headless-shell
```

> 说明：Playwright 默认 `headless: true` 使用独立的 `chrome-headless-shell` 二进制，所以上面同时安装了 `chromium` 和 `chromium-headless-shell`。

在 `Cargo.toml` 中添加依赖（本仓库即该 crate）：

```toml
[dependencies]
playwright = { git = "https://github.com/miloira/playwright-rs" }
tokio = { version = "1", features = ["full"] }
serde_json = "1"
```

---

## 快速开始

```rust,no_run
use playwright::Playwright;

#[tokio::main]
async fn main() -> playwright::Result<()> {
    let pw = Playwright::initialize().await?;      // 启动驱动并握手
    let chromium = pw.chromium().await?;
    let browser = chromium.launch(Default::default()).await?;

    let page = browser.new_page().await?;
    page.goto("https://example.com").await?;
    println!("标题: {}", page.title().await?);

    let shot = page.screenshot_png().await?;        // PNG 字节
    std::fs::write("example.png", &shot)?;

    browser.close(Default::default()).await?;
    pw.stop().await;                                // 关闭驱动进程
    Ok(())
}
```

---

## 两层 API

- **`playwright::api`（推荐）**：类型化、`snake_case` 的人体工学封装，覆盖全部 34 个协议接口。返回 channel 的命令直接给出类型化对象（如 `launch()` 返回 `Browser`），单字段结果自动解包（`title()` 返回 `String`）。另含手写便捷层：`Playwright` 入口、页面方法委托主 frame、`Locator`、`expect`、事件流、键鼠、路由等。
- **`playwright::protocol`（底层）**：从协议 spec 生成的完整通道层，每个命令一个方法、参数/返回/事件都类型化。需要 ergonomic 层未暴露的能力时使用。

从任意 ergonomic 包装可用 `.channel()` 拿到底层通道、`.connection()` 拿到连接。

---

## 启动与生命周期

```rust,no_run
# use playwright::Playwright;
# async fn f() -> playwright::Result<()> {
let pw = Playwright::initialize().await?;

let chromium = pw.chromium().await?;   // 也有 firefox() / webkit()
let browser = chromium.launch(Default::default()).await?;

// 带选项启动（协议参数通过类型化结构体传入）
use playwright::protocol::{BrowserTypeLaunchParams, LaunchOptions};
let browser2 = chromium.launch(BrowserTypeLaunchParams {
    mixin: LaunchOptions { headless: Some(false), ..Default::default() },
    ..Default::default()
}).await?;

let context = browser.new_context(Default::default()).await?;
let page = context.new_page().await?;      // 或 browser.new_page() 便捷方法

// 清理
page.close(Default::default()).await?;
context.close(Default::default()).await?;
browser.close(Default::default()).await?;
pw.stop().await;   // 结束驱动进程；也可在所有对象析构时自动结束
# Ok(()) }
```

驱动进程由连接持有：当最后一个派生对象被 drop 时自动结束；`pw.stop()` 可显式关闭。

---

## 导航与页面内容

```rust,no_run
# use playwright::Page;
# async fn f(page: Page) -> playwright::Result<()> {
let response = page.goto("https://example.com").await?;   // 返回 Option<Response>
page.set_content("<h1>hi</h1>").await?;                    // 直接设置 HTML（离线常用）

let html = page.content().await?;
let title = page.title().await?;
let url = page.url().await?;

page.click("#submit").await?;
page.fill("#name", "Ferris").await?;
let txt = page.inner_text("#heading").await?;
let maybe = page.text_content("#opt").await?;              // Option<String>
let href = page.get_attribute("a#link", "href").await?;    // Option<String>
page.wait_for_selector("#ready").await?;

let v = page.evaluate("() => navigator.userAgent").await?; // serde_json::Value
let sum = page.evaluate_with("(x) => x + 1", serde_json::json!(41)).await?;

let png = page.screenshot_png().await?;
# Ok(()) }
```

---

## Locator 定位器

Locator 是惰性的 `(frame, selector)` 句柄，每次操作都会重新解析。

```rust,no_run
# use playwright::Page;
# async fn f(page: Page) -> playwright::Result<()> {
let items = page.locator("li.item").await?;
println!("数量: {}", items.count().await?);
println!("第二项: {:?}", items.nth(1).text_content().await?);
for loc in items.all().await? {
    println!("{}", loc.inner_text().await?);
}

let input = page.locator("#name").await?;
input.fill("Ferris").await?;
input.clear().await?;
println!("值: {}", input.input_value().await?);

// get_by_* 语义化定位（Page / Frame / Locator 均有）
page.get_by_role("button", Some("提交")).await?.click().await?;
page.get_by_text("登录").await?.click().await?;
page.get_by_test_id("status").await?.inner_text().await?;
page.get_by_placeholder("邮箱").await?.fill("a@b.com").await?;

// iframe
let inside = page.frame_locator("iframe#editor").await?
    .locator("#body").inner_text().await?;
# let _ = inside; Ok(()) }
```

常用方法：`click` / `dblclick` / `hover` / `fill` / `clear` / `type_text` / `press` /
`check` / `uncheck` / `focus` / `tap` / `select_option` / `set_input_files` /
`drag_to` / `dispatch_event` / `scroll_into_view_if_needed`；查询
`count` / `text_content` / `inner_text` / `inner_html` / `input_value` /
`get_attribute` / `is_visible` 等 / `bounding_box` / `aria_snapshot` /
`all_text_contents` / `all_inner_texts` / `element_handle` / `screenshot_png` /
`evaluate`。

---

## expect 断言

Web-first 自动重试断言：轮询直到成立或超时（默认 5s），失败返回 `AssertionError`。

```rust,no_run
# use playwright::{expect, expect_page, Page};
# async fn f(page: Page) -> playwright::Result<()> {
let heading = page.locator("#heading").await?;
expect(&heading).to_be_visible().await?;
expect(&heading).to_have_text("Hello Playwright").await?;
expect(&heading).to_contain_text("Playwright").await?;
expect(&page.locator("li.item").await?).to_have_count(3).await?;
expect(&page.locator("#name").await?).to_have_value("Ferris").await?;
expect(&heading).not().to_be_hidden().await?;

// 自定义超时
use std::time::Duration;
expect(&heading).timeout(Duration::from_millis(500)).to_have_text("X").await.ok();

// 页面断言
expect_page(&page).to_have_title("首页").await?;
expect_page(&page).to_have_url("https://example.com/").await?;
# Ok(()) }
```

Locator 断言：`to_be_visible/hidden/attached/enabled/disabled/editable/focused/checked`、
`to_have_text` / `to_contain_text` / `to_have_value` / `to_have_attribute` / `to_have_count`。
另有 `expect_response(&api_response).to_be_ok()`。

---

## 事件

每个 `on_*` 返回 `tokio::sync::mpsc::UnboundedReceiver<T>`。console/dialog/request/response 等高频事件会自动开启订阅。

```rust,no_run
# use playwright::BrowserContext;
# async fn f(context: BrowserContext) -> playwright::Result<()> {
let mut console = context.on_console().await;    // ConsoleMessage { r#type, text }
let mut requests = context.on_request().await;   // Request
let mut responses = context.on_response().await; // Response
let mut dialogs = context.on_dialog().await;     // Dialog
let mut pages = context.on_page().await;         // Page（弹窗/新标签）

if let Some(msg) = console.recv().await {
    println!("[{}] {}", msg.r#type, msg.text);
}
# Ok(()) }
```

- 上下文：`on_console`、`on_dialog`、`on_dialog_closed`、`on_request`、`on_request_finished`、`on_request_failed`、`on_response`、`on_page`。
- 页面：`on_close`、`on_crash`、`on_worker`、`on_download`、`on_file_chooser`、`on_frame_attached`、`on_frame_detached`。
- `Download`：`save_as`、`path`、`failure`、`delete`、`cancel`。
- `FileChooser`：`set_files`、`element`、`is_multiple`。

---

## 输入设备：键盘 / 鼠标 / 触摸

```rust,no_run
# use playwright::Page;
# async fn f(page: Page) -> playwright::Result<()> {
page.keyboard().press("Enter").await?;
page.keyboard().type_text("hello").await?;
page.keyboard().down("Shift").await?;
page.keyboard().up("Shift").await?;

page.mouse().move_to(100.0, 200.0).await?;
page.mouse().click(100.0, 200.0).await?;
page.mouse().wheel(0.0, 240.0).await?;

page.touchscreen().tap(50.0, 50.0).await?;   // 需触摸上下文
# Ok(()) }
```

---

## 网络拦截 route

```rust,no_run
# use playwright::Page;
# async fn f(page: Page) -> playwright::Result<()> {
// 拦截所有请求，本地返回合成响应
page.route("**/*", |route| async move {
    let _ = route.fulfill_text(200, "text/html", "<title>Routed</title><h1>ok</h1>").await;
}).await?;

// 只拦截图片并中止
page.route("**/*.png", |route| async move {
    let _ = route.abort_now(Some("failed")).await;
}).await?;

// 放行并查看
page.route("**/api/**", |route| async move {
    println!("{}", route.url().await.unwrap_or_default());
    let _ = route.continue_().await;
}).await?;

page.goto("https://routed.test/").await?;
page.unroute(Some("**/*.png")).await?;   // 取消单个
page.unroute(None).await?;               // 取消全部
# Ok(()) }
```

- `Route` 便捷方法：`continue_`、`fulfill_text(status, content_type, body)`、`abort_now(code)`、`request()`、`url()`。
- 完整协议选项仍可用生成层的 `route.fulfill(params)` / `route.abort(params)`。
- 单一分发器按 glob（支持 `*` / `**` / `?`）匹配；未命中的请求会自动 `continue`，不会卡死。
- `context.route` / `context.unroute` 作用于整个上下文。

---

## Tracing / Clock / WebSocket / Dialog

```rust,no_run
# use playwright::BrowserContext;
# async fn f(context: BrowserContext) -> playwright::Result<()> {
// Tracing
let tracing = context.tracing().await?;
tracing.start(Some("run"), true, true).await?;   // (name, screenshots, snapshots)
// ... 执行操作 ...
tracing.stop(Some("trace.zip")).await?;           // 保存 trace 压缩包，可用 trace viewer 查看

// Clock（伪造时间）
let clock = context.clock();
clock.install().await?;
clock.set_fixed_time(1_700_000_000_000.0).await?; // 冻结 Date.now()
clock.fast_forward(5_000.0).await?;               // 快进 5 秒
# Ok(()) }
```

```rust,no_run
# use playwright::WebSocket;
# async fn f(ws: WebSocket) -> playwright::Result<()> {
let mut sent = ws.on_frame_sent().await;          // WebSocketFrame { opcode, payload }
let mut recv = ws.on_frame_received().await;
println!("url: {}", ws.url().await?);
# let _ = (&mut sent, &mut recv); Ok(()) }
```

Dialog：`dialog.message()`、`dialog.dialog_type()`、`dialog.accept_text(Some("prompt 内容"))`、`dialog.dismiss()`（生成层）。

---

## Request / Response

```rust,no_run
# use playwright::Response;
# async fn f(resp: Response) -> playwright::Result<()> {
println!("{} {}", resp.status().await?, resp.url().await?);
println!("ok = {}", resp.ok().await?);
let headers = resp.headers().await?;              // Vec<(String, String)>
let text = resp.text().await?;
let json = resp.json().await?;                    // serde_json::Value
let bytes = resp.body_bytes().await?;
# let _ = (headers, text, json, bytes); Ok(()) }
```

`Request`：`url`、`method`、`resource_type`、`is_navigation_request`、`headers`。

---

## 直接使用生成的协议层

需要 ergonomic 层未封装的命令时，直接用 `playwright::protocol`：

```rust,no_run
use playwright::protocol::{BrowserType, BrowserTypeLaunchParams, LaunchOptions};
use playwright::Playwright;

# async fn f() -> playwright::Result<()> {
let pw = Playwright::initialize().await?;
let conn = pw.connection();
let root = pw.channel();                           // protocol::Playwright
let init = root.initializer().await?;

let chromium = BrowserType::from_channel(conn.clone(), init.chromium.guid);
let launched = chromium.launch(BrowserTypeLaunchParams {
    mixin: LaunchOptions { headless: Some(true), ..Default::default() },
    ..Default::default()
}).await?;
let _browser_guid = launched.browser.guid;
# Ok(()) }
```

协议层覆盖全部接口（BrowserType/Browser/BrowserContext/Page/Frame/ElementHandle/JSHandle/Request/Response/Route/WebSocket/Worker/Dialog/Tracing/CDPSession/APIRequestContext/Android/Electron 等）、全部命令与事件。

---

## 驱动定位与环境变量

`Driver::locate` 的解析顺序：

1. `PLAYWRIGHT_CLI_JS`：驱动 `cli.js` 的完整路径。
2. `PLAYWRIGHT_DRIVER_DIR`：含 `node_modules/playwright[-core]` 的目录。
3. 当前工作目录下的 `driver/`。
4. crate 自带的 `driver/`（开发用）。

`PLAYWRIGHT_NODE` 可指定 `node` 可执行文件（默认用 PATH 上的 `node`）。

---

## 测试

```sh
cargo test
```

- 单元测试：`SerializedValue` 编解码、glob 匹配。
- 集成测试 `tests/integration.rs`：离线（`set_content`）驱动真实 Chromium，覆盖 Locator、断言、页面断言、事件、键鼠、路由、`get_by_*`、frame_locator、Tracing、Clock 等。若未安装驱动会自动跳过。

可运行示例：

```sh
cargo run --example handshake   # 仅握手，无需浏览器二进制
cargo run --example offline     # 离线 DOM/脚本/截图
cargo run --example locator     # Locator / expect / 事件
cargo run --example generated   # 直接使用协议层
cargo run --example basic       # 访问 example.com（需联网）
```

---

## 重新生成协议层

协议层从 `spec/*.yml`（锁定驱动版本）生成，已随仓库提交。升级 Playwright 版本后：

1. 更新 `spec/` 下的 yml（从对应 tag 的 `packages/protocol/spec/` 获取）。
2. 重新生成：

```sh
cargo run -p codegen
```

会写出 `src/protocol/generated.rs`（通道层）和 `src/api/generated.rs`（ergonomic 层）。

---

## 错误处理

所有异步 API 返回 `playwright::Result<T>`（即 `Result<T, playwright::Error>`）。`Error` 变体：

- `Io`：驱动进程或传输 I/O 错误。
- `Serde`：协议消息（反）序列化错误。
- `Driver { name, message, stack }`：驱动返回的服务端错误（如 `TimeoutError`，或断言失败的 `AssertionError`）。
- `ConnectionClosed`：命令进行中连接被关闭。
- `ObjectNotFound(guid)` / `Protocol(msg)`：本地对象缺失 / 协议消息异常。
