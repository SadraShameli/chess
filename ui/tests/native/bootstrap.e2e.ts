import { browser } from "@wdio/globals";
import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { describe, it } from "mocha";

describe("Native desktop bootstrap", () => {
  it("loads the local authoritative board and ready engine", async () => {
    const main = await browser.$("main");
    await main.waitForDisplayed();
    const engine = await browser.$("#engine-heading");
    await browser.waitUntil(async () => (await engine.getText()).includes("Reckless 0.9.0"));
    assert.match(await main.getText(), /Engine ready/);
    assert.match(await main.getText(), /No game has started/);
    const grid = await browser.$('[role="grid"]');
    await browser.waitUntil(async () => (await grid.getAttribute("aria-busy")) === "false");
    assert.equal((await browser.$$('[role="alert"]')).length, 0);
    assert.doesNotMatch(await main.getText(), /Loading 3D board/);
    const canvas = await browser.$("[data-board-canvas]");
    await canvas.waitForDisplayed();
    const size = await canvas.getSize();
    assert.ok(size.width > 0 && size.height > 0);
    const squares = await browser.$$('button[role="gridcell"]');
    assert.equal(squares.length, 64);
    const king = await browser.$('button[role="gridcell"][aria-label="e1, white king"]');
    assert.equal(await king.getAttribute("aria-label"), "e1, white king");
    const artifacts = resolve(import.meta.dirname, "../../test-results");
    await mkdir(artifacts, { recursive: true });
    await browser.saveScreenshot(resolve(artifacts, "native-bootstrap.png"));
  });
});
