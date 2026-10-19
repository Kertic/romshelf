# Setting up your Google client

Romshelf talks to Google Drive with your own Google Cloud client, so nothing is shared with anyone else. You do this once, on a computer, in about ten minutes.

You need a Google account. Use the same account as the Drive that will hold your ROMs.

## 1. Create a project

1. Open <https://console.cloud.google.com/> and sign in.
2. Open the project picker at the top of the page and choose **New project**.
3. Name it `Romshelf` and choose **Create**. Make sure the new project is selected in the project picker before going on.

## 2. Turn on the Drive API

1. Go to **APIs & Services → Library**.
2. Search for **Google Drive API**, open it and choose **Enable**.

## 3. Set up the sign-in screen

1. Go to **Google Auth Platform** (it may be listed as **OAuth consent screen**) and choose **Get started**.
2. **App information:** app name `Romshelf`, and your own email as the support email.
3. **Audience:** choose **External**.
4. **Contact information:** your email again.
5. Agree to the policy and choose **Create**.

## 4. Choose "Testing" or "In production"

Go to **Google Auth Platform → Audience**. Pick one:

- **Testing** (the default). Google ends every sign-in after seven days, so Romshelf asks you to sign in again about once a week. Under **Test users**, choose **Add users** and add your own Google address; without it, Google refuses the sign-in.
- **In production.** Under **Publishing status**, choose **Publish app** and confirm. Sign-ins then last until you sign out.

Either way, Google won't review the app, and that's fine for personal use: when you sign in, Google shows a "Google hasn't verified this app" page. Choose **Continue** (or **Advanced**, then **Go to Romshelf (unsafe)**).

When a weekly sign-in runs out, Romshelf says "Your Google sign-in has expired or was removed. Sign in again." Signing in may need Desktop Mode if Game Mode can't open a browser.

## 5. Create the Desktop client

1. Go to **Google Auth Platform → Clients** and choose **Create client**.
2. **Application type:** **Desktop app**. Name it `Romshelf Deck`.
3. Choose **Create**, then **Download JSON**. The file is named like `client_secret_123-abc.apps.googleusercontent.com.json`.

Keep this file private and never add it to the Romshelf repository (`.gitignore` already ignores `client_secret*.json`).

## 6. Give the client to Romshelf on the Deck

From a Windows PowerShell window on your PC, with SSH set up for the Deck (`ssh deck` works):

```powershell
scp $HOME\Downloads\client_secret_*.json deck:~/romshelf-client.json
```

```powershell
ssh deck "~/Applications/romshelf.AppImage auth set-client --file ~/romshelf-client.json && rm ~/romshelf-client.json"
```

Or on the Deck in Desktop Mode, copy the file over by any means and run in Konsole:

```bash
~/Applications/romshelf.AppImage auth set-client --file ~/Downloads/client_secret_*.json
```

Romshelf keeps the client in the system keyring, or in `~/.config/romshelf/token.json` (readable only by you) when no keyring is running.

## 7. Sign in

Open Romshelf and choose **Connect Google Drive**, or in Desktop Mode run:

```bash
~/Applications/romshelf.AppImage auth login
```

Sign in with your Google account and allow access to your Drive. If Game Mode can't open a browser, Romshelf shows the steps to finish in Desktop Mode.

Check the result at any time:

```bash
~/Applications/romshelf.AppImage auth status
```

## Why the full Drive permission

Romshelf asks to see and manage all of your Drive files. The narrower "only files this app created" permission can't see ROMs you upload through the Drive website or another device, which is the whole point. Romshelf only works inside the `Emulation` folder it creates, and never deletes anything permanently: removals go to Drive's trash.
