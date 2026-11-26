# Memory Measurements

Measurement tool: `scripts/ukur-memory.sh` (crate `memtool`). The numbers are physical footprint (`proc_pid_rusage`, `ri_phys_footprint`), the same as the Memory column in Activity Monitor. Helper processes are grouped under their app through the responsible pid.

Machine: arm64, 16 GB RAM, macOS 15.7.7, Safari 18.6.

## Attribution check (Task 2, 2026-11-15)

- `my-browser` (prototype, YouTube open): 9 processes, including 2 `com.apple.WebKit.WebContent`, `com.apple.WebKit.GPU`, and `com.apple.WebKit.Networking`. Attribution works.
- Safari (several of the user's tabs plus one Wikipedia tab): 14 processes, including 3 `com.apple.WebKit.WebContent`. Attribution works.

Observation: the prototype with one YouTube tab used two WebContent processes (601 MB and 328 MB). One of them is probably a stale process from the Google sign-in page that WebKit is still holding. This matches the risk in section 8 of the spec and needs attention in Task 9.

## Baseline: one YouTube tab (Task 2)

| Browser | Video | Total footprint | Process count | App process only |
|---|---|---|---|---|
| my-browser (prototype) | https://youtu.be/83H3YAm2woI, 1080p, measured at 18:32:58 after 2 minutes of playback | 1,415.4 MB | 10 | 36.8 MB |
| Safari 18.6 | same video, 1080p, measured at 18:36:22 after 2 minutes of playback | 1,812.3 MB | 17 | 58.8 MB |

Largest processes:

| Process | my-browser | Safari |
|---|---|---|
| YouTube tab WebContent | 839.4 MB | 1,487.5 MB |
| Other WebContent | 327.3 MB (one stale process, probably left over from the Google sign-in page) | 32.5 MB (three small processes) |
| WebKit GPU | 126.6 MB | 145.2 MB |
| App process | 36.8 MB | 58.8 MB |

Limits of this measurement: one sample each, taken 2 minutes after the video started playing. The video buffer can differ a lot between samples, so the numbers above cannot yet support a firm conclusion about the difference between the two browsers. What is fairly clear: the prototype's app process is smaller (36.8 MB versus 58.8 MB), and the 327 MB stale WebContent process in the prototype is memory that can be saved.

## Tab sleep and WebContent processes (Task 9, 2026-11-15)

Test setup: separate data folder (`MYBROWSER_DATA_DIR`), `sleep_idle_secs` 20, four tabs from the command line arguments (`id.wikipedia.org/wiki/Indonesia`, `github.com/servo/servo`, `example.com`, `www.rust-lang.org`), last tab active. Measured with `scripts/ukur-memory.sh --detail my-browser`.

| Time | WebContent processes | my-browser total |
|---|---|---|
| t+22 seconds, before the first tick | 4 | 399.0 MB |
| t+40 seconds, 10 seconds after the first tick put 3 tabs to sleep | 1 | 122.9 MB |

Bug found and fixed before the numbers above: pages created in `main()` before `app.run()` (restored session tabs and URLs from the arguments) were never freed. When released, their `WKWebView` still had 31 retains, because WebKit autoreleases the object dozens of times while creating it and there is no autorelease pool outside `app.run()`. As a result, the WebContent processes of sleeping tabs stayed alive (observed: 4 processes still alive after more than 120 seconds). Fix: all setup in `main()` is wrapped in `autoreleasepool` (`crates/app/src/main.rs`). After the fix, the retain count at release is 1 and the process exits.

CHECKPOINT decision: the WebContent processes of sleeping tabs exit within 10 seconds, so Task 12 (private API) is not needed.

## Ad blocker (Task 10, 2026-11-15)

Rule lists: EasyList + EasyPrivacy (downloaded 2026-11-15), converted by `blocklist-gen` into 131,509 WebKit rules in 3 lists (14 MB of JSON); 3,627 filters could not be converted and were skipped. First compile: 763, 1,183, and 1,265 ms per list (running in parallel); later launches use the compiled lists from WebKit storage.

One tab on `https://www.detik.com`, measured 30 seconds after the app opened, with a separate data folder for each run:

| Run | WebContent process | my-browser total |
|---|---|---|
| Blocker on, first compile | 142.2 MB | 190.8 MB |
| detik.com excepted (`blocker_exceptions`) | 455.1 MB | 529.6 MB |
| Blocker on, lists from cache | 118.0 MB | 162.6 MB |

One sample per run. Whether any part of the site looks broken has not been checked visually yet; that is on the manual checklist.

## Blocker fix: YouTube "Skip" button (2026-11-20)

User report: YouTube video ads had no "Skip" button.

Cause, proven in WebKit with a diagnostic program that loads a page with a given rule list and then inspects test elements injected into the page:

1. EasyList hides `.video-ads` on all sites (`##.video-ads`) and makes an exception for YouTube (`youtube.com#@#.video-ads`). The adblock crate turns that exception into a hide rule with `unless-domain`, while the generic rule still applies everywhere. As a result, the YouTube ad UI container, including the "Skip" button, was hidden as well, even though the ad video still played.
2. `@@||www.youtube.com^$generichide` (and 180 other sites, including GitHub, Gmail, and Wikipedia) is dropped by the adblock crate. As a result, 13,637 generic hide rules still applied on those sites.
3. Domains in cosmetic rules were written without a `*` prefix. In WebKit, `youtube.com` only matches the exact same host, so YouTube-specific rules did not apply on `www.youtube.com`.

A WebKit behavior that decided the shape of the fix: an `ignore-previous-rules` rule that only matches the document URL is not enough. For every subresource not covered by the exception (e.g. CSS from ytimg.com), WebKit installs the generic rules stylesheet again. So generichide is written as a single rule with `url-filter` `.*` and an `if-domain` that lists all of those sites, placed in a list reserved for generic hide rules.

Results (1 = hidden, 0 = shown; the last row counts resources loaded from ad networks):

| Test | No blocker | Old lists | New lists |
|---|---|---|---|
| YouTube, `.video-ads` element (Skip button container) | 0 | 1 | 0 |
| YouTube, generic selector `.video-ads-container` | 0 | 1 | 0 |
| YouTube, youtube.com-specific rule | 0 | 0 | 1 |
| GitHub, generic selector | 0 | 1 | 0 |
| detik.com, generic selector (site without an exception) | 0 | 1 | 1 |
| detik.com, loads from doubleclick/googlesyndication/adservice | 5 | 0 | 0 |

Limit: YouTube video ads still play, because they are served from the same servers as the video and the WebKit content blocker cannot tell them apart. This fix only brings back the "Skip" button for ads that can be skipped.

Another bug found during this testing: removing an old rule list from WebKit storage with an empty completion handler crashed the process (SIGSEGV in `removeContentRuleListForIdentifier:completionHandler:`). This happened every time the rule list contents changed, including when these new lists were installed. Fixed, and covered by a smoke test.

## Sidebar redesign, stage 1 (2026-11-26)

Release builds of `main` (0.1.1, the tab strip UI) and of this stage, each in a separate test instance (temporary `HOME` and data folder). Numbers come from the MCP `memory_usage` tool, which attributes processes to one instance by pid. `scripts/ukur-memory.sh` groups by app name, which would also count the owner's own running browser.

**Memory, 20 tabs** (15 `example.com` pages and 5 Wikipedia articles), `sleep_idle_secs` 20, measured after the background tabs slept (median of 3 samples per run):

| Run | App process, `main` | App process, stage 1 | WebContent processes = awake tabs |
|---|---|---|---|
| 1 | 36.3 MB | 29.3 MB | yes (1 = 1) in both |
| 2 | 26.1 MB | 28.6 MB | yes |
| 3 | 31.1 MB | 32.1 MB | yes |

The gate (stage 1 at most 15 MB above `main`) passes. The new chrome adds no WebContent process.

The first measurement of stage 1 used **89.0 MB**. `footprint` showed 44 MB of IOSurface (0 in `main`). Every view drawn in `drawRect` had its own full-size bitmap: the gradient, the card shadow, the empty state, and the outline. After moving them to Core Animation layer properties (`CAGradientLayer`, layer background and border colors, `shadowPath`), IOSurface went to 0. One tab then measured a total phys_footprint of 24 MB (`main`: 20 MB).

**CPU: not conclusive yet.** Two automated attempts could not produce a fair comparison:

- **YouTube** (same video, 60 s of `top` samples): the results varied too much between runs. In one `main` run the video did not play. In one stage 1 run there were spikes from ad or quality changes. Steady-state app plus WebContent was about 3.2% (`main`) against 3.7% and 8% (stage 1).
- **Local full-screen canvas animation at 60 fps:** the `main` test window was apparently covered, so WebKit paused the animation (0% WebContent). For stage 1 the app used 1.4–2.8% and WebContent 1.3–2.8%.

A manual comparison with both windows visible is in the manual checklist.
