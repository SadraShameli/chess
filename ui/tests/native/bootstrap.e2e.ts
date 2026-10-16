import { browser } from "@wdio/globals";
import assert from "node:assert/strict";
import { describe, it } from "mocha";

describe("Native desktop bootstrap", () => {
  it("loads the local application shell", async () => {
    const main = await browser.$("main");
    await main.waitForDisplayed();
    assert.equal(await main.getText(), "Reckless Chess");
  });
});
