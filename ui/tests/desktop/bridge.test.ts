import { describe, expect, it } from "vitest";
import { DesktopBridge } from "@/desktop/bridge";
import { DesktopBridgeError } from "@/desktop/bridge_error";
import { DesktopCommandError } from "@/desktop/command_error";
import fixture from "@/desktop/generated/fixture.json";
import type { DesktopRequest } from "@/desktop/generated/contracts";
import type { DesktopTransportProvider } from "@/desktop/transport_provider";
import { DesktopValidation } from "@/desktop/validation";
import { DesktopValidationError } from "@/desktop/validation_error";

class DesktopTransportFixture implements DesktopTransportProvider {
  readonly requests: DesktopRequest[] = [];

  constructor(
    readonly response: unknown,
    readonly events: readonly unknown[] = [],
  ) {}

  request(request: DesktopRequest, events?: (event: unknown) => void): Promise<unknown> {
    this.requests.push(request);
    for (const event of this.events) {
      events?.(event);
    }
    return Promise.resolve(this.response);
  }
}

describe("Typed native bridge", () => {
  const validation = new DesktopValidation();
  const requestId = () => fixture.request.request_id;

  it("validates both boundaries and correlates the native response", async () => {
    const transport = new DesktopTransportFixture(fixture.response);
    const bridge = new DesktopBridge(transport, validation, requestId);
    expect((await bridge.bootstrap()).board.pieces).toHaveLength(32);
    expect(transport.requests[0]?.payload.operation).toBe("bootstrap");
    const foreign = new DesktopBridge(
      new DesktopTransportFixture({
        ...fixture.response,
        request_id: "123e4567-e89b-42d3-a456-426614174001",
      }),
      validation,
      requestId,
    );
    await expect(foreign.bootstrap()).rejects.toBeInstanceOf(DesktopBridgeError);
    const malformed = new DesktopBridge(
      new DesktopTransportFixture({ protocol_version: 99 }),
      validation,
      requestId,
    );
    await expect(malformed.bootstrap()).rejects.toBeInstanceOf(DesktopValidationError);
  });

  it("preserves typed command failures without treating them as success", async () => {
    const response = {
      ...fixture.response,
      result: {
        status: "failure",
        error: {
          code: "resource_refusal",
          message: "No CPU capacity is available",
          retryable: true,
        },
      },
    };
    const bridge = new DesktopBridge(new DesktopTransportFixture(response), validation, requestId);
    await expect(bridge.bootstrap()).rejects.toBeInstanceOf(DesktopCommandError);
  });

  it("validates channel events and reports an invalid event without applying it", async () => {
    const response = {
      ...fixture.response,
      result: {
        status: "success",
        revision: null,
        value: {
          operation: "stream_subscribe",
          value: {
            subscription_id: fixture.event.subscription_id,
            sequence: "0",
            snapshot: fixture.response.result.value.value,
          },
        },
      },
    };
    const transport = new DesktopTransportFixture(response, [
      fixture.event,
      { protocol_version: 999 },
    ]);
    const bridge = new DesktopBridge(transport, validation, requestId);
    const received: string[] = [];
    const errors: Error[] = [];
    const result = await bridge.subscribe({
      event: (event) => {
        received.push(event.sequence);
      },
      error: (error) => {
        errors.push(error);
      },
    });
    expect(result.sequence).toBe("0");
    expect(received).toEqual(["1"]);
    expect(errors).toHaveLength(1);
    expect(errors[0]).toBeInstanceOf(DesktopValidationError);
  });
});
