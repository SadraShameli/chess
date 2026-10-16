import { markRaw, shallowRef } from "vue";
import type { DeepReadonly, ShallowRef } from "vue";
import type { DesktopBridge } from "@/desktop/bridge";
import type { DesktopBootstrap, DesktopEvent } from "@/desktop/generated/contracts";
import { DesktopSnapshotEnumAcceptance } from "@/desktop/snapshot";
import type { DesktopSnapshot } from "@/desktop/snapshot";

export enum DesktopApplicationEnumPhase {
  Loading,
  Ready,
  Failed,
}

export interface DesktopApplicationState {
  readonly phase: DesktopApplicationEnumPhase;
  readonly snapshot: DeepReadonly<DesktopBootstrap> | null;
  readonly message: string | null;
}

export class DesktopApplication {
  readonly #bridge: DesktopBridge;
  readonly #snapshots: DesktopSnapshot;
  readonly #state: ShallowRef<DesktopApplicationState>;
  #subscriptionId: string | null = null;
  #pending: Promise<void> = Promise.resolve();
  #buffer: DesktopEvent[] | null = [];
  #generation = 0;
  #acknowledged = 0n;
  #acknowledging: Promise<void> | null = null;

  constructor(bridge: DesktopBridge, snapshots: DesktopSnapshot) {
    this.#bridge = bridge;
    this.#snapshots = snapshots;
    this.#state = shallowRef({
      phase: DesktopApplicationEnumPhase.Loading,
      snapshot: null,
      message: null,
    });
    markRaw(this);
  }

  get state(): Readonly<ShallowRef<DesktopApplicationState>> {
    return this.#state;
  }

  async start(): Promise<void> {
    const generation = ++this.#generation;
    this.#buffer = [];
    this.#subscriptionId = null;
    this.#pending = Promise.resolve();
    this.#acknowledging = null;
    this.#acknowledged = 0n;
    this.#update(DesktopApplicationEnumPhase.Loading);
    try {
      const receipt = await this.#bridge.subscribe({
        event: (event) => {
          if (generation !== this.#generation) return;
          if (this.#buffer) {
            if (this.#buffer.length === DesktopApplication.#maximumBufferedEvents) {
              this.#buffer = null;
              this.#snapshots.requireResync();
              this.#fail("The native event stream needs to reconnect.");
              return;
            }
            this.#buffer.push(event);
          } else {
            this.#enqueue(event);
          }
        },
        error: () => {
          if (generation !== this.#generation) return;
          this.#buffer = null;
          this.#snapshots.requireResync();
          this.#fail("An invalid native event was rejected. Refresh the position to reconnect.");
        },
      });
      if (generation !== this.#generation) return;
      this.#subscriptionId = receipt.subscription_id;
      this.#snapshots.begin(receipt.subscription_id, receipt.snapshot, receipt.sequence);
      const buffered = this.#takeBuffered();
      this.#update(DesktopApplicationEnumPhase.Ready);
      if (buffered === null) {
        await this.refresh();
      } else {
        for (const event of buffered) this.#enqueue(event);
        await this.#pending;
      }
    } catch {
      if (generation === this.#generation) {
        this.#fail("Native application state is unavailable. Retry to reconnect.");
      }
    }
  }

  async refresh(): Promise<void> {
    const generation = this.#generation;
    const subscriptionId = this.#subscriptionId;
    if (subscriptionId === null) {
      await this.start();
      return;
    }
    try {
      const receipt = await this.#bridge.resynchronize(subscriptionId);
      if (generation !== this.#generation || subscriptionId !== this.#subscriptionId) return;
      const acceptance = this.#snapshots.resynchronize(receipt);
      if (acceptance === DesktopSnapshotEnumAcceptance.Applied) {
        await this.#acknowledge(subscriptionId, receipt.sequence, generation);
        if (generation !== this.#generation || subscriptionId !== this.#subscriptionId) return;
        this.#update(DesktopApplicationEnumPhase.Ready);
      } else if (acceptance !== DesktopSnapshotEnumAcceptance.Duplicate) {
        this.#fail("The native position could not be refreshed. Retry to reconnect.");
      }
    } catch {
      if (generation !== this.#generation || subscriptionId !== this.#subscriptionId) return;
      this.#fail("The native position could not be refreshed. Retry to reconnect.");
    }
  }

  dispose(): void {
    this.#generation += 1;
    this.#subscriptionId = null;
    this.#buffer = null;
  }

  async settled(): Promise<void> {
    let pending;
    do {
      pending = this.#pending;
      await pending;
    } while (pending !== this.#pending);
  }

  static readonly #maximumBufferedEvents = 64;

  #takeBuffered(): DesktopEvent[] | null {
    const buffered = this.#buffer;
    this.#buffer = null;
    return buffered;
  }

  #enqueue(event: DesktopEvent): void {
    const generation = this.#generation;
    this.#pending = this.#pending
      .then(() => this.#accept(event, generation))
      .catch(() => {
        if (generation !== this.#generation) return;
        this.#snapshots.requireResync();
        this.#fail("The native event stream was interrupted. Refresh the position to reconnect.");
      });
  }

  async #accept(event: DesktopEvent, generation: number): Promise<void> {
    if (generation !== this.#generation) return;
    const acceptance = this.#snapshots.accept(event);
    switch (acceptance) {
      case DesktopSnapshotEnumAcceptance.Applied:
      case DesktopSnapshotEnumAcceptance.StaleRevision:
        this.#update(DesktopApplicationEnumPhase.Ready);
        await this.#acknowledge(event.subscription_id, this.#snapshots.sequence, generation);
        break;
      case DesktopSnapshotEnumAcceptance.Failure:
        if (event.payload.kind === "failure") this.#fail(event.payload.value.message);
        await this.#acknowledge(event.subscription_id, this.#snapshots.sequence, generation);
        break;
      case DesktopSnapshotEnumAcceptance.ResyncRequired:
        await this.refresh();
        break;
      case DesktopSnapshotEnumAcceptance.Duplicate:
      case DesktopSnapshotEnumAcceptance.ForeignSubscription:
        break;
    }
  }

  async #acknowledge(subscriptionId: string, sequence: string, generation: number): Promise<void> {
    while (this.#acknowledging !== null) await this.#acknowledging;
    if (generation !== this.#generation || subscriptionId !== this.#subscriptionId) return;
    const delivered = BigInt(sequence);
    if (delivered <= this.#acknowledged) return;
    const pending = this.#bridge.acknowledge(subscriptionId, sequence);
    this.#acknowledging = pending;
    try {
      await pending;
      if (generation === this.#generation) this.#acknowledged = delivered;
    } finally {
      if (this.#acknowledging === pending) this.#acknowledging = null;
    }
  }

  #update(phase: DesktopApplicationEnumPhase, message: string | null = null): void {
    this.#state.value = { phase, snapshot: this.#snapshots.current, message };
  }

  #fail(message: string): void {
    this.#update(DesktopApplicationEnumPhase.Failed, message);
  }
}
