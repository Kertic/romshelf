import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Keyboard, TextField } from "./Keyboard";

afterEach(cleanup);
const button = (name: string | RegExp) => screen.getByRole("button", { name });

describe("on-screen keyboard", () => {
  it("types, capitalises once, spaces and deletes", () => {
    const onDone = vi.fn();
    render(<Keyboard label="Name in Steam" value="Kirby" onDone={onDone} onCancel={vi.fn()} />);
    fireEvent.click(button("Space"));
    fireEvent.click(button("Shift"));
    // With Shift on, the keys show capitals.
    fireEvent.click(button("D"));
    fireEvent.click(button("x"));
    fireEvent.click(button("x"));
    fireEvent.click(button("Delete"));
    fireEvent.click(button("Done"));
    expect(onDone).toHaveBeenCalledWith("Kirby Dx");
  });

  it("a text field opens it from the button beside it, and keeps the result", () => {
    const onChange = vi.fn();
    render(<TextField label="Name in Steam" value="Kirby" onChange={onChange} />);
    fireEvent.click(button("Type name in steam with the controller"));
    expect(screen.getByRole("dialog", { name: "Name in Steam" })).toBeTruthy();
    fireEvent.click(button("1"));
    fireEvent.click(button("Done"));
    expect(onChange).toHaveBeenCalledWith("Kirby1");
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
