import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import App from "@/App.vue";
import { DesktopApplicationFixture } from "@tests/desktop/application_fixture";

describe("Application shell", () => {
  it("renders the native starting position and verified engine without active game controls", async () => {
    const fixture = new DesktopApplicationFixture();
    const view = mount(App, {
      props: { application: fixture.application },
      global: { stubs: { BoardView: true } },
    });
    await flushPromises();
    expect(view.get("main").text()).toContain("Reckless Chess");
    expect(view.text()).toContain("Reckless 0.9.0");
    expect(view.text()).toContain("Engine ready");
    expect(view.text()).toContain("No game has started.");
    expect(view.findComponent({ name: "BoardView" }).attributes("disabled")).toBe("");
    expect(view.findAll("button").map((button) => button.text())).toEqual(["Refresh position"]);
    view.unmount();
  });

  it("shows native connection failure with an explicit retry", async () => {
    const fixture = new DesktopApplicationFixture();
    fixture.unavailable = true;
    const view = mount(App, {
      props: { application: fixture.application },
      global: { stubs: { BoardView: true } },
    });
    await flushPromises();
    expect(view.get('[role="alert"]').text()).toContain("Retry to reconnect");
    fixture.unavailable = false;
    await view.get('[role="alert"] button').trigger("click");
    await flushPromises();
    expect(view.text()).toContain("Engine ready");
    view.unmount();
  });
});
