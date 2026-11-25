# Networking: HTTP/3 disabled

my-browser uses HTTP/2 (over TCP), not HTTP/3 (QUIC over UDP). The setting is `http3_enabled` in `settings.json`, and its default is `false` (`crates/core/src/settings.rs`).

## Reason

On 2026-11-25, Hostinger Mail showed a white page after login. An investigation through MCP in the affected tab found:

- The login succeeded (the `auth/me` API returned 200), but about 170 of the app's JS and CSS files failed to load in the same millisecond, then the app stopped ("Router initialization failed") and reloaded itself.
- Those files exist on the server, their code passes the Safari JavaScript engine's syntax check, and the ad blocker does not block them.
- The connection used HTTP/3. From that tab, 30 simultaneous requests to Hostinger static files all failed, and even sequential requests sometimes failed.
- From the same Mac, 60 simultaneous requests over HTTP/2 (curl) all succeeded.
- my-browser used HTTP/3 because of an `Alt-Svc` hint from Cloudflare stored in `~/Library/Caches/local.my-browser/WebKit/AlternativeServices/AlternativeService.sqlite`. After that hint was deleted, the Inbox opened immediately, then went white again once the hint was recorded again.

The conclusion: the user's network (a CGNAT gateway, no VPN in the path) is unreliable for QUIC when many requests are sent at once. Safari is not affected, and the exact reason in Safari was not investigated.

## How it works

WebKit has no public switch or SPI to disable HTTP/3 in WKWebView on macOS 15.7 (checked at runtime). What is available is the website data type `_WKWebsiteDataTypeAlternativeServices`. When `http3_enabled` is off, `Engine::forget_http3_hints` removes that data from the default data store, both in the network process memory and in its file (`crates/engine-webkit/src/lib.rs`):

- when the browser opens;
- on every tick (30 seconds);
- before every navigation and before a tab is woken (`crates/core/src/browser.rs`).

This way new connections never use HTTP/3. Connections already in progress are not affected.

## Verification

Two test instances, each opening mail.hostinger.com, closed, then opened again:

| Setting | Protocol on second load |
|---|---|
| `http3_enabled: true` | h3 |
| default (`false`) | h2 |

Core tests: `http3_hints_forgotten_at_start_tick_and_navigation_when_disabled` and `http3_hints_kept_when_enabled` (`crates/core/src/browser/tests.rs`).

## Not yet verified, and limitations

- The HTTP/3 failure did not occur in the test instances during verification (30 of 30 requests succeeded), so the problem is intermittent. The evidence of the failure comes from the user's tab.
- Sites that advertise HTTP/3 through DNS (HTTPS/SVCB records) may still use HTTP/3, because that does not go through `Alt-Svc`. mail.hostinger.com is not one of them.
- If the network becomes reliable, HTTP/3 can be turned back on with `"http3_enabled": true` (quit the browser before editing `settings.json`).
