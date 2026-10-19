import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ConnectionStatus, Folder, SetupDone, UiError } from "./api";
import { FirstRun, type BackAction, type FirstRunApi } from "./FirstRun";

afterEach(cleanup);

const signedOut: ConnectionStatus = {
  client_configured: true,
  signed_in: false,
  account: null,
  drive_folder: null,
  ready: false,
};
const signedIn: ConnectionStatus = { ...signedOut, signed_in: true, account: "me@example.com" };
const ready: ConnectionStatus = { ...signedIn, drive_folder: "My Drive / Emulation", ready: true };

const FOLDERS: Record<string, Folder[]> = {
  root: [
    { id: "games", name: "Games" },
    { id: "photos", name: "Photos" },
  ],
  games: [
    { id: "retro", name: "Retro" },
    { id: "emu", name: "Emulation" },
  ],
};

const DONE: SetupDone = { path: "My Drive / Emulation", tree: ["Emulation/", "  roms/      snes, gba"] };

function fakeApi(overrides: Partial<FirstRunApi> = {}) {
  let progress: ((p: string) => void) | null = null;
  const api: FirstRunApi = {
    connectionStatus: vi.fn(async () => signedIn),
    authLogin: vi.fn(async () => "me@example.com"),
    authCancel: vi.fn(async () => {}),
    driveFolders: vi.fn(async (parent: string) => FOLDERS[parent] ?? []),
    driveSetup: vi.fn(async () => {
      progress?.("Emulation");
      progress?.("Emulation/roms");
      return DONE;
    }),
    onSetupProgress: vi.fn(async (h: (p: string) => void) => {
      progress = h;
      return () => {
        progress = null;
      };
    }),
    ...overrides,
  };
  return api;
}

async function setup(status: ConnectionStatus, api: FirstRunApi) {
  const onDone = vi.fn();
  let back: BackAction | null = null;
  render(<FirstRun status={status} onDone={onDone} setBack={(b) => (back = b)} api={api} />);
  await act(async () => {});
  return { onDone, back: () => back };
}

async function click(name: string | RegExp) {
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name }));
  });
}

describe("first run", () => {
  it("welcome leads to sign-in", async () => {
    await setup(signedOut, fakeApi());
    expect(screen.getByText("Step 1 of 4")).toBeTruthy();
    expect(screen.getByText(/doesn.t include any games/)).toBeTruthy();
    await click("Connect Google Drive");
    expect(screen.getByText("Step 2 of 4")).toBeTruthy();
    expect(screen.getByText("A browser will open for Google sign-in.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Open browser" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeTruthy();
  });

  it("waits for sign-in, then lists My Drive", async () => {
    let finish: (v: string) => void = () => {};
    const api = fakeApi({ authLogin: vi.fn(() => new Promise<string>((r) => (finish = r))) });
    const { back } = await setup(signedOut, api);
    await click("Connect Google Drive");
    await click("Open browser");
    expect(screen.getByText("Waiting for sign-in…")).toBeTruthy();
    expect(back()?.label).toBe("Cancel");
    await act(async () => finish("me@example.com"));
    expect(screen.getByText("Step 3 of 4")).toBeTruthy();
    expect(api.driveFolders).toHaveBeenCalledWith("root");
    expect(screen.getByText("Games")).toBeTruthy();
    expect(screen.getByText("My Drive")).toBeTruthy();
  });

  it("shows Desktop Mode steps when no browser opens", async () => {
    const err: UiError = { kind: "no_browser", message: "No browser could be opened." };
    await setup(signedOut, fakeApi({ authLogin: vi.fn(async () => Promise.reject(err)) }));
    await click("Connect Google Drive");
    await click("Open browser");
    expect(screen.getByText(/Switch to Desktop/)).toBeTruthy();
    expect(screen.getByText("~/Applications/romshelf.AppImage auth login")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
  });

  it("offers the Desktop Mode steps while waiting", async () => {
    const api = fakeApi({ authLogin: vi.fn(() => new Promise<string>(() => {})) });
    await setup(signedOut, api);
    await click("Connect Google Drive");
    await click("Open browser");
    await click(/see a browser/);
    expect(api.authCancel).toHaveBeenCalled();
    expect(screen.getByText(/Switch to Desktop/)).toBeTruthy();
  });

  it("shows sign-in errors inline with a retry", async () => {
    const err: UiError = { kind: "offline", message: "Couldn't reach Google. Check the Deck's internet connection and try again." };
    await setup(signedOut, fakeApi({ authLogin: vi.fn(async () => Promise.reject(err)) }));
    await click("Connect Google Drive");
    await click("Open browser");
    expect(screen.getByText(err.message)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
  });

  it("explains set-client when no Google client is stored", async () => {
    const api = fakeApi({ connectionStatus: vi.fn(async () => signedOut) });
    await setup({ ...signedOut, client_configured: false }, api);
    await click("Connect Google Drive");
    expect(screen.getByText(/auth set-client --file/)).toBeTruthy();
    await click("Check again");
    expect(screen.getByRole("button", { name: "Open browser" })).toBeTruthy();
  });

  it("browses into folders and B goes up", async () => {
    const api = fakeApi();
    const { back } = await setup(signedIn, api);
    expect(screen.getByText("Step 3 of 4")).toBeTruthy();
    expect(back()).toBeNull();
    await click(/Games/);
    expect(api.driveFolders).toHaveBeenLastCalledWith("games");
    expect(screen.getByText("My Drive / Games")).toBeTruthy();
    expect(screen.getByText(/already has an Emulation folder/)).toBeTruthy();
    expect(back()?.label).toBe("Up a folder");
    await act(async () => back()!.run());
    expect(screen.getByText("My Drive")).toBeTruthy();
    expect(screen.queryByText(/already has an Emulation folder/)).toBeNull();
  });

  it("creates the folder tree and shows it", async () => {
    const api = fakeApi();
    const { onDone } = await setup(signedIn, api);
    await click(/Games/);
    await click("Create Emulation folder here");
    expect(api.driveSetup).toHaveBeenCalledWith("games", "My Drive / Games");
    expect(screen.getByText("Step 4 of 4")).toBeTruthy();
    expect(screen.getByText(/roms\/\s+snes, gba/)).toBeTruthy();
    await click("Open Library");
    expect(onDone).toHaveBeenCalled();
  });

  it("keeps the user on the location step when setup fails", async () => {
    const err: UiError = { kind: "needs_update", message: "The settings file was written by a newer Romshelf. Please update Romshelf." };
    await setup(signedIn, fakeApi({ driveSetup: vi.fn(async () => Promise.reject(err)) }));
    await click("Create Emulation folder here");
    expect(screen.getByText("Step 3 of 4")).toBeTruthy();
    expect(screen.getByText(err.message)).toBeTruthy();
  });

  it("goes straight to the library when the account already has a folder", async () => {
    const api = fakeApi({ connectionStatus: vi.fn(async () => ready) });
    const { onDone } = await setup(signedOut, api);
    await click("Connect Google Drive");
    await click("Open browser");
    expect(onDone).toHaveBeenCalled();
  });
});
