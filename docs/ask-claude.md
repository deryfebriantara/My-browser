# Ask Claude: chat beside the page

A chat panel on the right side of the window for asking about the page you have open, for example "summarize this article", "compare it with the tab next to it", or "why does this button look broken on mobile?". Answers come from Claude Code on your Mac, so they use the same Claude subscription quota, with no API key.

## How to use

- Open the panel with the ✦ button at the top right of the sidebar, the **Claude > Ask Claude** menu, or ⌘J. Close it the same way.
- Type a question and press Enter (Shift+Enter for a new line). The **Stop** button or Esc stops the answer.
- **New** starts a new conversation.
- Every question automatically carries the title and URL of the active tab, so "this page" always means the tab you are looking at.
- To ask about one element: pick the element first with **Claude > Pick Element for Claude** (⌘⇧E), then ask "explain the element I picked".

## Account and sign-in

- The bar below the panel title shows the account in use: email, plan (for example Max or Pro), and Claude Code profile. If there are several profiles (`~/.claude`, `~/.claude-work`, and so on), choose one there. Switching profiles starts a new conversation, because Claude Code stores sessions per profile.
- If that profile is not signed in, the panel shows a **Sign In to Claude** card. The options are **Sign In with Claude Subscription**, or **Use API Console (Pay per Use)**, which is billed per use.
  - The sign-in page opens in a **my-browser tab**, not in Safari or Chrome. Sign in and approve there; the panel continues on its own and the sign-in tab closes automatically.
  - If the sign-in page shows a code, paste it into the "Does the sign-in page show a code?" field.
- **Sign Out** signs that profile out of Claude Code. Claude Code in Terminal using the same profile is signed out as well, so the panel asks for confirmation before it does this.

How it works: the panel runs `claude auth login` for the selected profile. The CLI opens the sign-in page with the `open` command. For that sign-in process only, `open` is replaced through PATH with a small script that hands the URL to my-browser. After approval, the page redirects to localhost, where the CLI is waiting. Account status is read from `claude auth status --json`.

## What Claude can and cannot do in the panel

Can: read page text, screenshot tabs, list tabs, search history (`search_history`), inspect elements and the picked element, read the console and the network log, scroll, control YouTube videos.

Cannot: open URLs or tabs, click, type into the page, run JavaScript, change files, run terminal commands, or browse the web. Claude runs without its built-in tools (`--tools ""`), only with the my-browser read tools, and any other tool is denied automatically (`--permission-prompts none`).

The reason: page text that Claude reads can contain malicious instructions. With read-only access, such instructions cannot make Claude open another site (for example to send the page contents out) or touch your files. The panel itself is locked down too: it loads nothing from the internet (CSP `default-src 'none'`), and links in answers open in a new tab, not in the panel.

## Memory

The Claude Code process (about 270 MB) only starts when the first message is sent, and is stopped when the panel closes. The chat history is still shown when the panel is opened again, and the conversation continues with `claude --resume`. While the panel is closed, no Claude process runs. The history is lost when the browser quits.

## Settings (`settings.json`)

| Key | Default | Purpose |
|---|---|---|
| `claude_config_dir` | `~/.claude` | Claude Code profile (`CLAUDE_CONFIG_DIR`), for example `~/.claude-work` |
| `claude_path` | detected automatically | Location of the `claude` binary. Searched for in `~/.local/bin`, `~/.claude/local`, Homebrew, then nvm (latest Node version) |
| `claude_model` | Claude Code's default model | For example `sonnet` for faster answers |

File location: `~/Library/Application Support/my-browser/settings.json`. Quit the browser before editing.

## If "Not logged in" or "Failed to authenticate" appears

That profile has no login yet, or the login has expired. The panel checks the status again and shows the sign-in card. To do it by hand: run `CLAUDE_CONFIG_DIR=~/.claude claude` in Terminal (adjust the profile), then type `/login`. The panel always sets `CLAUDE_CONFIG_DIR` on purpose, because without that variable Claude Code reads the default login, which can differ from the profile you use in Terminal.

## How it works

```
panel (local WKWebView) ─JSON message─▶ my-browser ──stdin/stdout stream-json──▶ claude -p
                                                                                   │
                                          my-browser --mcp ◀──── read tools ───────┘
```

- `crates/core/src/claude_chat.rs`: CLI arguments, tab context, event translator, tool labels.
- `crates/engine-webkit/src/panel.rs`: panel webview (no blocker, no stored data, cannot navigate away).
- `crates/app/src/chat.rs`: Claude process, history, session, messages to and from the panel.
- `crates/app/src/chat_account.rs`: account status, sign-in (`claude auth login` with the `open` replacement), sign-out, profile switching.
- `crates/app/src/chat_panel.html`: panel UI and the safe Markdown renderer.

For testing, debug builds have a `MYBROWSER_CHAT_SELFTEST=<file>` hook: each time the panel opens, the next line of that file is sent, and the transcript is written to `<file>.out`. Lines that start with `!` are sent as panel messages, for example `!{"t":"login","method":"claudeai"}`.
