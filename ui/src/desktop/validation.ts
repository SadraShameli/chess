import {
  validateDesktopRequest,
  validateDesktopResponse,
  validateDesktopEvent,
} from "@resources/desktop/validators.mjs";
import type { DesktopEvent, DesktopRequest, DesktopResponse } from "@/desktop/generated/contracts";
import { DesktopValidationError } from "@/desktop/validation_error";

export class DesktopValidation {
  request(value: unknown): DesktopRequest {
    if (!validateDesktopRequest(value)) {
      throw new DesktopValidationError("request");
    }
    return value;
  }

  response(value: unknown): DesktopResponse {
    if (!validateDesktopResponse(value)) {
      throw new DesktopValidationError("response");
    }
    return value;
  }

  event(value: unknown): DesktopEvent {
    if (!validateDesktopEvent(value)) {
      throw new DesktopValidationError("event");
    }
    return value;
  }
}
