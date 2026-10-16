import type { DesktopResponse } from "@/desktop/generated/contracts";

type DesktopCommandFailure = Extract<DesktopResponse["result"], { status: "failure" }>["error"];

export class DesktopCommandError extends Error {
  constructor(readonly failure: DesktopCommandFailure) {
    super(failure.message);
    this.name = "DesktopCommandError";
  }
}
