import { DesktopBridgeEnumFailure, DesktopBridgeError } from "@/desktop/bridge_error";
import { DesktopCommandError } from "@/desktop/command_error";
import { desktopProtocolVersion } from "@/desktop/generated/contracts";
import type {
  DesktopBootstrap,
  DesktopRequest,
  DesktopResponse,
  DesktopStreamSnapshot,
} from "@/desktop/generated/contracts";
import type { DesktopStreamObserver, DesktopTransportProvider } from "@/desktop/transport_provider";
import type { DesktopValidation } from "@/desktop/validation";
import { DesktopValidationError } from "@/desktop/validation_error";

type DesktopBridgeSuccess = Extract<DesktopResponse["result"], { status: "success" }>;

export class DesktopBridge {
  readonly #transport: DesktopTransportProvider;
  readonly #validation: DesktopValidation;
  readonly #requestId: () => string;

  constructor(
    transport: DesktopTransportProvider,
    validation: DesktopValidation,
    requestId: () => string,
  ) {
    this.#transport = transport;
    this.#validation = validation;
    this.#requestId = requestId;
  }

  async request(
    payload: DesktopRequest["payload"],
    expectedRevision: DesktopRequest["expected_revision"] = null,
    observer?: DesktopStreamObserver,
  ): Promise<DesktopBridgeSuccess> {
    const request = this.#validation.request({
      protocol_version: desktopProtocolVersion,
      request_id: this.#requestId(),
      expected_revision: expectedRevision,
      payload,
    });
    let raw: unknown;
    try {
      raw = await this.#transport.request(
        request,
        observer
          ? (value) => {
              this.#event(value, observer);
            }
          : undefined,
      );
    } catch (cause) {
      throw new DesktopBridgeError(DesktopBridgeEnumFailure.Unavailable, { cause });
    }
    const response = this.#validation.response(raw);
    if (response.request_id !== request.request_id) {
      throw new DesktopBridgeError(DesktopBridgeEnumFailure.Correlation);
    }
    if (response.result.status === "failure") {
      throw new DesktopCommandError(response.result.error);
    }
    if (response.result.value.operation !== payload.operation) {
      throw new DesktopBridgeError(DesktopBridgeEnumFailure.Operation);
    }
    return response.result;
  }

  async bootstrap(): Promise<DesktopBootstrap> {
    const result = await this.request({ operation: "bootstrap" });
    if (result.value.operation !== "bootstrap") {
      throw new DesktopBridgeError(DesktopBridgeEnumFailure.Operation);
    }
    return result.value.value;
  }

  async subscribe(observer: DesktopStreamObserver): Promise<DesktopStreamSnapshot> {
    const result = await this.request({ operation: "stream_subscribe" }, null, observer);
    if (result.value.operation !== "stream_subscribe") {
      throw new DesktopBridgeError(DesktopBridgeEnumFailure.Operation);
    }
    return result.value.value;
  }

  async acknowledge(subscriptionId: string, sequence: string): Promise<void> {
    await this.request({
      operation: "stream_acknowledge",
      subscription_id: subscriptionId,
      sequence,
    });
  }

  async resynchronize(subscriptionId: string): Promise<DesktopStreamSnapshot> {
    const result = await this.request({
      operation: "stream_resync",
      subscription_id: subscriptionId,
    });
    if (result.value.operation !== "stream_resync") {
      throw new DesktopBridgeError(DesktopBridgeEnumFailure.Operation);
    }
    return result.value.value;
  }

  #event(value: unknown, observer: DesktopStreamObserver): void {
    let event;
    try {
      event = this.#validation.event(value);
    } catch (error) {
      if (error instanceof DesktopValidationError) {
        observer.error(error);
        return;
      }
      throw error;
    }
    observer.event(event);
  }
}
