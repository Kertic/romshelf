# Releasing Romshelf

Romshelf on the Deck checks GitHub once a day when its interface opens. When there is a newer signed release, it downloads it beside `~/Applications/romshelf.AppImage`, checks it, and swaps it in with a rename. Steam shortcuts keep working because the path never changes. The new version runs from the next start, and the old one is kept as `romshelf.AppImage.previous`.

## One-time setup

### 1. Create the signing key

From Git Bash in the repository, signed in to `gh`:

```bash
scripts/new-update-key.sh
```

This writes the public key to `crates/romshelf-core/update-key.pub`, stores the private key in the repository's `UPDATE_SIGNING_KEY` secret, and saves a backup copy at `~/romshelf-update-signing-key.pem`. Commit the `.pub` file. Move the backup into a password manager and delete it from the computer.

Builds without a public key never update themselves.

### 2. Let the Deck read releases

While the repository is private, the Deck needs a token to read releases:

1. On GitHub, go to **Settings → Developer settings → Fine-grained tokens → Generate new token**.
2. **Repository access:** only `Kertic/romshelf`. **Permissions → Contents:** Read-only. Pick an expiry you're comfortable with.
3. On the Deck (or over SSH), run the command below and paste the token when asked:

```bash
~/Applications/romshelf.AppImage update --set-token
```

If the repository becomes public, skip this.

### 3. Install a build that can update itself

The first build with the public key has to be installed by hand, the same way as before (`scp` it to `~/Applications/romshelf.AppImage`). Every release after that installs itself.

## Each release

1. Bump `version` under `[workspace.package]` in `Cargo.toml` (and `ui/package.json` to match), and commit on `main`.
2. Tag and push:

```bash
git tag v0.2.0
```

```bash
git push origin v0.2.0
```

The **Release** workflow builds the AppImage with the usual CI checks, refuses a tag that doesn't match `Cargo.toml`, writes `romshelf-update.json` (version, SHA-256, size), signs it, checks the signature against the committed public key, and publishes the release with all three files.

## On the Deck

- Romshelf updates by itself when its interface opens, at most once a day, and never while a game is launching. A line in the top bar says when a new version is ready.
- Update right away: `~/Applications/romshelf.AppImage update`
- Go back to the previous version: `~/Applications/romshelf.AppImage update --rollback`. The version you rolled back from isn't installed again automatically; the next newer release is.
