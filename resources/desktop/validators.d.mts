import type { DesktopRequest, DesktopResponse, DesktopEvent } from "@/desktop/generated/contracts";
export function validateDesktopRequest(value: unknown): value is DesktopRequest;
export function validateDesktopResponse(value: unknown): value is DesktopResponse;
export function validateDesktopEvent(value: unknown): value is DesktopEvent;
