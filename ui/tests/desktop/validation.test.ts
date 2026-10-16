import { describe, expect, it } from "vitest";
import fixture from "@/desktop/generated/fixture.json";
import { DesktopValidation } from "@/desktop/validation";
import { DesktopValidationError } from "@/desktop/validation_error";

describe("Rust desktop wire contracts", () => {
  const validation = new DesktopValidation();

  it("validates real Rust requests, responses and events without losing u64 precision", () => {
    const request = validation.request(fixture.request);
    expect(request.expected_revision).toBe("18446744073709551615");
    const response = validation.response(fixture.response);
    expect(response.result.status).toBe("success");
    if (response.result.status !== "success" || response.result.value.operation !== "bootstrap") {
      throw new Error("Missing Rust bootstrap fixture");
    }
    expect(response.result.value.value.board.pieces).toHaveLength(32);
    expect(response.result.value.value.board.legal_moves).toHaveLength(20);
    expect(validation.event(fixture.event).sequence).toBe("1");
  });

  it("rejects unknown fields, unsupported protocols and unknown operations", () => {
    expect(() => validation.request({ ...fixture.request, extra: true })).toThrow(
      DesktopValidationError,
    );
    expect(() => validation.request({ ...fixture.request, protocol_version: 2 })).toThrow(
      DesktopValidationError,
    );
    expect(() =>
      validation.request({ ...fixture.request, payload: { operation: "shell" } }),
    ).toThrow(DesktopValidationError);
    expect(() => validation.event({ ...fixture.event, extra: true })).toThrow(
      DesktopValidationError,
    );
  });

  it("rejects malformed identities and unsigned decimal counters", () => {
    for (const request_id of [
      "00000000-0000-0000-0000-000000000000",
      fixture.request.request_id.toUpperCase(),
      "/tmp/selected.pgn",
    ]) {
      expect(() => validation.request({ ...fixture.request, request_id })).toThrow(
        DesktopValidationError,
      );
    }
    for (const expected_revision of [0, "01", "-1", "18446744073709551616"]) {
      expect(() => validation.request({ ...fixture.request, expected_revision })).toThrow(
        DesktopValidationError,
      );
    }
  });

  it("validates bounded native board data and CPU capacity", () => {
    const response = fixture.response;
    const value = response.result.value.value;
    expect(() =>
      validation.response({
        ...response,
        result: {
          ...response.result,
          value: { ...response.result.value, value: { ...value, cpu_threads: 0 } },
        },
      }),
    ).toThrow(DesktopValidationError);
    expect(() =>
      validation.response({
        ...response,
        result: {
          ...response.result,
          value: {
            ...response.result.value,
            value: {
              ...value,
              board: { ...value.board, pieces: [{ color: "white", kind: "king", square: 64 }] },
            },
          },
        },
      }),
    ).toThrow(DesktopValidationError);
  });
});
