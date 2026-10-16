import { describe, expect, it } from "vitest";
import fixture from "@/desktop/generated/fixture.json";
import { DesktopSnapshot, DesktopSnapshotEnumAcceptance } from "@/desktop/snapshot";
import { DesktopValidation } from "@/desktop/validation";

describe("Authoritative desktop snapshots", () => {
  const validation = new DesktopValidation();

  it("applies ordered events and refuses duplicate, stale and foreign updates", () => {
    const initial = validation.event(fixture.event);
    if (initial.payload.kind !== "snapshot") {
      throw new Error("Missing Rust snapshot fixture");
    }
    const store = new DesktopSnapshot();
    store.begin(initial.subscription_id, initial.payload.value);
    expect(store.accept(initial)).toBe(DesktopSnapshotEnumAcceptance.Applied);
    expect(store.accept(initial)).toBe(DesktopSnapshotEnumAcceptance.Duplicate);
    const newer = validation.event({
      ...fixture.event,
      sequence: "2",
      payload: {
        kind: "snapshot",
        value: { ...initial.payload.value, revision: "18446744073709551615" },
      },
    });
    expect(store.accept(newer)).toBe(DesktopSnapshotEnumAcceptance.Applied);
    const stale = validation.event({ ...fixture.event, sequence: "3" });
    expect(store.accept(stale)).toBe(DesktopSnapshotEnumAcceptance.StaleRevision);
    expect(store.current?.revision).toBe("18446744073709551615");
    const foreign = validation.event({
      ...fixture.event,
      subscription_id: "123e4567-e89b-42d3-a456-426614174001",
      sequence: "4",
    });
    expect(store.accept(foreign)).toBe(DesktopSnapshotEnumAcceptance.ForeignSubscription);
    expect(store.sequence).toBe("3");
  });

  it("requires explicit resync after a gap and preserves the last accepted snapshot", () => {
    const initial = validation.event(fixture.event);
    if (initial.payload.kind !== "snapshot") {
      throw new Error("Missing Rust snapshot fixture");
    }
    const store = new DesktopSnapshot();
    store.begin(initial.subscription_id, initial.payload.value);
    store.accept(initial);
    const gap = validation.event({ ...fixture.event, sequence: "3" });
    expect(store.accept(gap)).toBe(DesktopSnapshotEnumAcceptance.ResyncRequired);
    expect(store.sequence).toBe("1");
    expect(store.accept(validation.event({ ...fixture.event, sequence: "2" }))).toBe(
      DesktopSnapshotEnumAcceptance.ResyncRequired,
    );
    expect(
      store.resynchronize({
        subscription_id: initial.subscription_id,
        sequence: "3",
        snapshot: initial.payload.value,
      }),
    ).toBe(DesktopSnapshotEnumAcceptance.Applied);
    expect(store.sequence).toBe("3");
    expect(store.needsResync).toBe(false);
  });
});
