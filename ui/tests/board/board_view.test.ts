import { mount } from "@vue/test-utils";
import { userEvent } from "vitest/browser";
import { afterEach, describe, expect, it, vi } from "vitest";
import BoardView from "@/board/BoardView.vue";
import { BoardTestFixture } from "@tests/board/fixture";
import "@/theme/tokens.css";

describe("Accessible 3D board surface", () => {
  const fixture = new BoardTestFixture();
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("keeps all 64 native square labels and keyboard navigation available after graphics failure", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    const view = mount(BoardView, {
      props: { snapshot: fixture.snapshot, disabled: true },
      attachTo: document.body,
    });
    await vi.waitFor(() => {
      expect(view.find('[role="alert"]').exists()).toBe(true);
    });
    const squares = view.findAll('[role="gridcell"]');
    expect(squares).toHaveLength(64);
    expect(view.get('[data-square="4"]').attributes("aria-label")).toBe("e1, white king");
    expect(view.get('[role="grid"]').attributes("aria-disabled")).toBe("true");
    const a1 = view.get('[data-square="0"]');
    if (!(a1.element instanceof HTMLButtonElement)) {
      throw new Error("Missing semantic square button");
    }
    a1.element.focus();
    await a1.trigger("keydown", { key: "ArrowUp" });
    expect(document.activeElement).toBe(view.get('[data-square="8"]').element);
    await view.get('[data-square="8"]').trigger("click");
    expect(view.emitted("square")).toBeUndefined();
    expect(view.emitted("failure")).toEqual([
      ["The 3D board requires WebGL2 graphics acceleration."],
    ]);
    expect(view.get("[data-board-canvas]").attributes("aria-hidden")).toBe("true");
    view.unmount();
  });

  it("emits explicit keyboard square intents after real WebGL initialization", async () => {
    const view = mount(BoardView, {
      props: { snapshot: fixture.snapshot },
      attachTo: document.body,
    });
    try {
      await vi.waitFor(() => {
        expect(view.emitted("ready")).toEqual([[]]);
      });
      const square = view.get('[data-square="12"]').element;
      if (!(square instanceof HTMLButtonElement)) {
        throw new Error("Missing semantic square button");
      }
      square.focus();
      await userEvent.keyboard("{ArrowUp}{Enter}");
      expect(document.activeElement).toBe(view.get('[data-square="20"]').element);
      expect(view.emitted("square")).toEqual([[20]]);
      expect(view.emitted("failure")).toBeUndefined();
      await view.setProps({ disabled: true });
      await userEvent.keyboard("{Enter}");
      expect(view.emitted("square")).toEqual([[20]]);
    } finally {
      view.unmount();
    }
  });
});
