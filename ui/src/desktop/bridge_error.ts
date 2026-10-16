export enum DesktopBridgeEnumFailure {
  Correlation = "The native response belongs to a different request",
  Operation = "The native response belongs to a different operation",
  Unavailable = "The native request could not be confirmed. Refresh its state before retrying",
}

export class DesktopBridgeError extends Error {
  constructor(
    readonly reason: DesktopBridgeEnumFailure,
    options?: ErrorOptions,
  ) {
    super(reason, options);
    this.name = "DesktopBridgeError";
  }
}
