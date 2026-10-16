import { DesktopApplication } from "@/desktop/application";
import { DesktopBridge } from "@/desktop/bridge";
import fixture from "@/desktop/generated/fixture.json";
import type { DesktopBootstrap, DesktopEvent, DesktopRequest } from "@/desktop/generated/contracts";
import { DesktopSnapshot } from "@/desktop/snapshot";
import type { DesktopTransportProvider } from "@/desktop/transport_provider";
import { DesktopValidation } from "@/desktop/validation";

export class DesktopApplicationFixture implements DesktopTransportProvider {
  readonly requests: DesktopRequest[] = [];
  readonly application: DesktopApplication;
  readonly snapshot: DesktopBootstrap;
  unavailable = false;
  initialEvents: DesktopEvent[] = [];
  resyncWait: Promise<void> | null = null;
  rejectResync = false;
  rejectAcknowledgement = false;
  invalidInitialEvent = false;
  #events: ((event: unknown) => void) | undefined;
  #sequence = "0";
  readonly #validation = new DesktopValidation();

  constructor() {
    const response = this.#validation.response(fixture.response);
    if (response.result.status !== "success" || response.result.value.operation !== "bootstrap") {
      throw new Error("The native bootstrap fixture is missing");
    }
    this.snapshot = {
      ...response.result.value.value,
      engine: {
        state: "ready",
        identity: {
          name: "Reckless 0.9.0",
          author: "Reckless contributors",
          supported_options: ["Threads"],
        },
      },
    };
    this.application = new DesktopApplication(
      new DesktopBridge(this, this.#validation, () => crypto.randomUUID()),
      new DesktopSnapshot(),
    );
  }

  async request(request: DesktopRequest, events?: (event: unknown) => void): Promise<unknown> {
    this.requests.push(request);
    if (this.unavailable) return Promise.reject(new Error("Native transport unavailable"));
    let value;
    switch (request.payload.operation) {
      case "stream_subscribe":
        this.#events = events;
        if (this.invalidInitialEvent) this.#events?.({ protocol_version: 999 });
        for (const event of this.initialEvents) this.#events?.(event);
        value = {
          operation: "stream_subscribe",
          value: {
            subscription_id: fixture.event.subscription_id,
            sequence: this.#sequence,
            snapshot: this.snapshot,
          },
        };
        break;
      case "stream_resync":
        if (this.resyncWait) await this.resyncWait;
        if (this.rejectResync) throw new Error("Delayed resync failed");
        this.#sequence = (BigInt(this.#sequence) + 1n).toString();
        value = {
          operation: "stream_resync",
          value: {
            subscription_id: fixture.event.subscription_id,
            sequence: this.#sequence,
            snapshot: this.snapshot,
          },
        };
        this.#events?.(this.event(this.#sequence));
        break;
      case "stream_acknowledge":
        if (this.rejectAcknowledgement) throw new Error("Acknowledgement failed");
        value = { operation: "stream_acknowledge" };
        break;
      case "bootstrap":
        value = { operation: "bootstrap", value: this.snapshot };
        break;
      case "file_select":
        value = { operation: "file_select", value: null };
        break;
    }
    return Promise.resolve({
      protocol_version: fixture.response.protocol_version,
      request_id: request.request_id,
      result: { status: "success", revision: null, value },
    });
  }

  event(sequence: string): DesktopEvent {
    return this.#validation.event({
      ...fixture.event,
      sequence,
      payload: { kind: "snapshot", value: this.snapshot },
    });
  }

  emit(event: DesktopEvent): void {
    this.#events?.(event);
  }
}
