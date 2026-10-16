import { Channel, invoke } from "@tauri-apps/api/core";
import type { DesktopRequest } from "@/desktop/generated/contracts";
import type { DesktopTransportProvider } from "@/desktop/transport_provider";

export class DesktopTransportNative implements DesktopTransportProvider {
  async request(request: DesktopRequest, events?: (event: unknown) => void): Promise<unknown> {
    const onEvent = events ? new Channel<unknown>(events) : null;
    return invoke<unknown>("desktop_command", { request, onEvent });
  }
}
