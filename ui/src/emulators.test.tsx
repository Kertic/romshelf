import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { EmulatorView } from "./api";
import { Emulators, rowAction, rowStatus } from "./Emulators";

afterEach(cleanup);

const RA: EmulatorView = {
  id: "retroarch",
  name: "RetroArch",
  systems: ["SNES", "GBA", "N64"],
  status: "installed",
  status_label: "Installed",
  setting_up: false,
};
const DOLPHIN: EmulatorView = {
  id: "dolphin",
  name: "Dolphin",
  systems: ["GameCube"],
  status: "not_installed",
  status_label: "Not installed",
  setting_up: false,
};

describe("Emulators screen", () => {
  it("lists each emulator with its systems, status and action", () => {
    const onSetup = vi.fn();
    render(<Emulators list={[RA, DOLPHIN]} error={null} setups={{}} onSetup={onSetup} onBack={vi.fn()} />);
    expect(screen.getByRole("heading", { name: "Emulators" })).toBeTruthy();
    expect(screen.getByText("SNES, GBA, N64")).toBeTruthy();
    expect(screen.getByText("Not installed")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Set up" }));
    expect(onSetup).toHaveBeenCalledWith("dolphin", false);
    fireEvent.click(screen.getByRole("button", { name: "Repair" }));
    expect(onSetup).toHaveBeenCalledWith("retroarch", true);
    expect(screen.getByText(/never changes the settings you or other tools/)).toBeTruthy();
  });

  it("shows progress while setting up and the reason when it fails", () => {
    expect(rowStatus(DOLPHIN, { step: "Installing", percent: 30, running: true, message: null, failed: false })).toEqual({
      text: "Installing… 30%",
      warn: false,
    });
    const failed = { step: null, percent: null, running: false, failed: true, message: "Couldn't install Dolphin." };
    expect(rowStatus(DOLPHIN, failed)).toEqual({ text: "Couldn't install Dolphin.", warn: true });
    expect(rowAction(DOLPHIN, failed)).toBe("Try again");
    expect(rowStatus({ ...RA, status: "setup_incomplete", status_label: "Setup incomplete" })).toEqual({
      text: "Setup incomplete",
      warn: true,
    });
  });

  it("disables the button while that emulator is being set up", () => {
    render(
      <Emulators
        list={[{ ...DOLPHIN, setting_up: true }]}
        error={null}
        setups={{}}
        onSetup={vi.fn()}
        onBack={vi.fn()}
      />,
    );
    expect((screen.getByRole("button", { name: "Set up" }) as HTMLButtonElement).disabled).toBe(true);
  });
});
