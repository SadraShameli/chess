import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import App from "@/App.vue";

describe("Application shell", () => {
  it("renders an accessible application landmark", () => {
    const view = mount(App);
    expect(view.get("main").text()).toContain("Reckless Chess");
    view.unmount();
  });
});
