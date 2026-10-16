import { describe, expect, it } from "vitest";
import { DesktopApplicationEnumPhase } from "@/desktop/application";
import { DesktopApplicationFixture } from "@tests/desktop/application_fixture";

class DesktopApplicationDeferred {
  readonly promise: Promise<void>;
  #resolve: (() => void) | null = null;

  constructor() {
    this.promise = new Promise((resolve) => {
      this.#resolve = resolve;
    });
  }

  resolve(): void {
    if (this.#resolve === null) throw new Error("Missing deferred resolver");
    this.#resolve();
  }
}

describe("Application stream ownership", () => {
  it("uses the validated native receipt and buffers events arriving before subscribe completes", async () => {
    const fixture = new DesktopApplicationFixture();
    fixture.initialEvents.push(fixture.event("1"));
    await fixture.application.start();
    expect(fixture.application.state.value.phase).toBe(DesktopApplicationEnumPhase.Ready);
    expect(fixture.application.state.value.snapshot?.board.pieces).toHaveLength(32);
    expect(fixture.requests.map((request) => request.payload.operation)).toEqual([
      "stream_subscribe",
      "stream_acknowledge",
    ]);
    fixture.application.dispose();
  });

  it("recovers sequence gaps and acknowledges the fresh boundary", async () => {
    const fixture = new DesktopApplicationFixture();
    await fixture.application.start();
    fixture.emit(fixture.event("4"));
    await fixture.application.settled();
    expect(fixture.requests.map((request) => request.payload.operation)).toEqual([
      "stream_subscribe",
      "stream_resync",
      "stream_acknowledge",
    ]);
    expect(fixture.application.state.value.phase).toBe(DesktopApplicationEnumPhase.Ready);
    fixture.application.dispose();
  });

  it("preserves the last board on transport failure and reconnects only on request", async () => {
    const fixture = new DesktopApplicationFixture();
    await fixture.application.start();
    fixture.unavailable = true;
    await fixture.application.refresh();
    expect(fixture.application.state.value.phase).toBe(DesktopApplicationEnumPhase.Failed);
    expect(fixture.application.state.value.snapshot?.board.pieces).toHaveLength(32);
    fixture.unavailable = false;
    await fixture.application.start();
    expect(fixture.application.state.value.phase).toBe(DesktopApplicationEnumPhase.Ready);
    fixture.application.dispose();
  });
});

describe("Application subscription generations", () => {
  it("resynchronizes an invalid event received before the subscribe receipt", async () => {
    const fixture = new DesktopApplicationFixture();
    fixture.invalidInitialEvent = true;
    await fixture.application.start();
    await fixture.application.settled();
    expect(fixture.requests.map((request) => request.payload.operation)).toEqual([
      "stream_subscribe",
      "stream_resync",
      "stream_acknowledge",
    ]);
    expect(fixture.application.state.value.phase).toBe(DesktopApplicationEnumPhase.Ready);
  });

  it("ignores a failed old resync after reconnection or disposal", async () => {
    const fixture = new DesktopApplicationFixture();
    await fixture.application.start();
    const waiting = new DesktopApplicationDeferred();
    fixture.resyncWait = waiting.promise;
    const refresh = fixture.application.refresh();
    await fixture.application.start();
    fixture.rejectResync = true;
    waiting.resolve();
    await refresh;
    expect(fixture.application.state.value.phase).toBe(DesktopApplicationEnumPhase.Ready);
    fixture.rejectResync = false;
    const disposedWait = new DesktopApplicationDeferred();
    fixture.resyncWait = disposedWait.promise;
    const disposedRefresh = fixture.application.refresh();
    fixture.application.dispose();
    const before = fixture.application.state.value;
    fixture.rejectResync = true;
    disposedWait.resolve();
    await disposedRefresh;
    expect(fixture.application.state.value).toBe(before);
  });
});

describe("Application acknowledgement recovery", () => {
  it("can refresh after a rejected acknowledgement without poisoning the next attempt", async () => {
    const fixture = new DesktopApplicationFixture();
    await fixture.application.start();
    fixture.rejectAcknowledgement = true;
    fixture.emit(fixture.event("1"));
    await fixture.application.settled();
    expect(fixture.application.state.value.phase).toBe(DesktopApplicationEnumPhase.Failed);
    fixture.rejectAcknowledgement = false;
    await fixture.application.refresh();
    await fixture.application.settled();
    expect(fixture.application.state.value.phase).toBe(DesktopApplicationEnumPhase.Ready);
    fixture.application.dispose();
  });
});
