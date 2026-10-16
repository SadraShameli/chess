import type { DesktopEvent, DesktopRequest } from "@/desktop/generated/contracts";

export interface DesktopStreamObserver {
  event(event: DesktopEvent): void;
  error(error: Error): void;
}

export interface DesktopTransportProvider {
  request(request: DesktopRequest, events?: (event: unknown) => void): Promise<unknown>;
}
