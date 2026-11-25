# Password manager

my-browser stores passwords in an encrypted vault that can only be opened with your master password.

## How to use

- **Save:** sign in on a site as usual. After the form is submitted, the offer "Save Password for github.com?" appears. The choices are **Save**, **Not Now**, or **Never for This Site**. The first time you save, you are asked to create a master password (at least 10 characters).
- **Fill:** on a login page, press ⌘\ or choose **Passwords > Fill Password**. If there are several accounts for that site, pick one. Multi-step logins (for example Google): press ⌘\ at the username step, then again at the password step.
- **Manage:** **Passwords > Manage Passwords…** (⌥⌘P) to search, view, copy, add, edit, and delete passwords, import and export CSV, and change the master password.
- **Random password:** **Passwords > Copy Random Password** copies a 20-character password to the clipboard, for example for a sign-up form. In the Passwords window, the **Generate** button fills the password field.
- **Lock:** **Passwords > Lock Passwords**.

## Encryption

| Part | Method |
|---|---|
| Master password → key | Argon2id, 256 MiB, 3 iterations, random 16-byte salt (about 0.3 seconds on an Apple Silicon Mac). That much memory makes mass guessing with GPUs very expensive |
| Vault key | 32 random bytes, wrapped with XChaCha20-Poly1305 using the key derived from the master password |
| Vault contents | Encrypted with XChaCha20-Poly1305 using the vault key, with a fresh random 24-byte nonce on every save |
| Integrity | The file version and parameters are bound as associated data; a file changed even slightly fails to open |
| File | `~/Library/Application Support/my-browser/vault.json`, permissions 0600, written atomically. No sites, usernames, or passwords in plain text |
| Libraries | RustCrypto (`argon2`, `chacha20poly1305`, `zeroize`) |

The vault key is wrapped separately, so changing the master password does not require re-encrypting the contents. Another unlock method (Touch ID through the Secure Enclave) will only need one more wrapping later.

## Other protections

- **Lookalike sites:** passwords are offered only for the exact same origin (scheme, host, port). `github.com` does not match `github.com.evil.example`, `http://github.com`, or `www.github.com`.
- **No autofill:** filling happens only after you ask for it (⌘\), never on page load. This way, hidden forms cannot siphon passwords. Filling happens only in the main frame, and the page origin is checked again right before filling.
- **Page scripts cannot interfere:** the login capturer and the filler run in a separate WebKit content world. Site scripts cannot see their code or send fake messages. WebKit determines the saving origin from the frame, not from the page contents.
- **Automatic locking:** after 10 minutes unused (`password_lock_minutes`), when the screen sleeps or is locked, on user switch, and when the Mac sleeps. When locked, the key and entries are cleared from memory.
- **Clipboard:** copied passwords are marked "concealed" so clipboard managers do not record them, and are cleared after 30 seconds if the clipboard has not changed.
- **Export and master password change** always ask for the master password again, even while the vault is unlocked.
- **Claude cannot access the vault:** there is no MCP tool or socket command for passwords. `run_js` is refused on pages whose password field is filled, and `list_elements` never sends the contents of password fields.
- The master password field uses NSSecureTextField: while it is active, other apps cannot read the keystrokes.

## Limitations to know about

- **The master password cannot be recovered.** If you forget it, the vault contents are lost. Keep it somewhere safe.
- A page that has been filled with a password can read that value. This applies to every password manager.
- Clearing secrets from memory is best effort: Rust and WebKit can keep temporary copies that cannot be forcibly erased. A password revealed with **Show** is hidden again after 20 seconds.
- There is no sync to other devices yet. Passkeys are not supported yet (they need a special Apple entitlement for browsers). Login forms inside iframes are not filled yet.
- While the vault is locked, saved accounts are recognized from the last unlock (in memory only). After the browser restarts, the first login can show the save offer again; choosing **Save** asks for the master password and then reports that the password is already saved.
- Hosts in "Never for This Site" are stored in `settings.json` without encryption (site names only).

## Importing from other browsers

In the **Passwords > Manage Passwords…** window, the **Import CSV…** button accepts exports from Chrome, Safari/the Passwords app (File > Export), Firefox, and Bitwarden. For an existing account (same site and username), the password is updated. After importing, delete that CSV file (also from the Trash), because it contains unencrypted passwords.

## Plan: Touch ID

Touch ID that actually protects the key (through the Secure Enclave) requires an app signed with an Apple Developer certificate with the keychain entitlement. Ad-hoc signed apps are refused with error `-34018` (already tried). Once a certificate is available (a free "Personal Team" account in Xcode is probably enough for personal use), the vault key can be wrapped with a Secure Enclave key as a second unlock method, with the master password kept as the fallback.

## Settings (`settings.json`)

| Key | Default | Purpose |
|---|---|---|
| `password_offer_save` | `true` | Offer to save after login |
| `password_lock_minutes` | `10` | Lock after this many minutes unused |
| `password_never_save` | `[]` | Hosts that are never offered |

## Code

- `crates/vault`: file format, cryptography, entries, generator, CSV (no macOS dependencies).
- `crates/core/src/password_capture.js`, `password_fill.js`: login capturer and filler.
- `crates/engine-webkit/src/passwords.rs`: receiver for messages from the password content world.
- `crates/app/src/passwords.rs`: save offer, fill, auto-lock, clipboard.
- `crates/app/src/password_manager.rs` and `.html`: the Manage Passwords window.
- `crates/app/src/dialogs.rs`: master password and confirmation dialogs.

For testing, debug builds have a `MYBROWSER_DIALOG_SELFTEST=<master>` hook that answers every dialog automatically with that master password. This hook does not exist in release builds.
