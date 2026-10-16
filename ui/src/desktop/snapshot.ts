import type { DeepReadonly } from "vue";
import type {
  DesktopBootstrap,
  DesktopEvent,
  DesktopStreamSnapshot,
} from "@/desktop/generated/contracts";

export enum DesktopSnapshotEnumAcceptance {
  Applied,
  Duplicate,
  StaleRevision,
  ForeignSubscription,
  ResyncRequired,
  Failure,
}

export class DesktopSnapshot {
  #current: DeepReadonly<DesktopBootstrap> | null = null;
  #subscriptionId: string | null = null;
  #sequence = 0n;
  #needsResync = true;

  get current(): DeepReadonly<DesktopBootstrap> | null {
    return this.#current;
  }

  get sequence(): string {
    return this.#sequence.toString();
  }

  get needsResync(): boolean {
    return this.#needsResync;
  }

  begin(subscriptionId: string, snapshot: DesktopBootstrap, sequence = "0"): void {
    this.#subscriptionId = subscriptionId;
    this.#current = snapshot;
    this.#sequence = BigInt(sequence);
    this.#needsResync = false;
  }

  requireResync(): void {
    this.#needsResync = true;
  }

  accept(event: DesktopEvent): DesktopSnapshotEnumAcceptance {
    if (event.subscription_id !== this.#subscriptionId) {
      return DesktopSnapshotEnumAcceptance.ForeignSubscription;
    }
    if (this.#needsResync) {
      return DesktopSnapshotEnumAcceptance.ResyncRequired;
    }
    const sequence = BigInt(event.sequence);
    if (sequence <= this.#sequence) {
      return DesktopSnapshotEnumAcceptance.Duplicate;
    }
    if (sequence !== this.#sequence + 1n) {
      this.requireResync();
      return DesktopSnapshotEnumAcceptance.ResyncRequired;
    }
    this.#sequence = sequence;
    switch (event.payload.kind) {
      case "snapshot":
        return this.#apply(event.payload.value);
      case "failure":
        return DesktopSnapshotEnumAcceptance.Failure;
    }
  }

  resynchronize(receipt: DesktopStreamSnapshot): DesktopSnapshotEnumAcceptance {
    if (receipt.subscription_id !== this.#subscriptionId) {
      return DesktopSnapshotEnumAcceptance.ForeignSubscription;
    }
    if (BigInt(receipt.sequence) < this.#sequence) {
      return DesktopSnapshotEnumAcceptance.Duplicate;
    }
    const result = this.#apply(receipt.snapshot);
    if (result !== DesktopSnapshotEnumAcceptance.Applied) {
      return result;
    }
    this.#sequence = BigInt(receipt.sequence);
    this.#needsResync = false;
    return result;
  }

  #apply(snapshot: DesktopBootstrap): DesktopSnapshotEnumAcceptance {
    if (this.#current && BigInt(snapshot.revision) < BigInt(this.#current.revision)) {
      return DesktopSnapshotEnumAcceptance.StaleRevision;
    }
    this.#current = snapshot;
    return DesktopSnapshotEnumAcceptance.Applied;
  }
}
