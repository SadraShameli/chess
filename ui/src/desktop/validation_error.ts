export class DesktopValidationError extends Error {
  constructor(boundary: string) {
    super(`The native ${boundary} does not match the supported desktop protocol`);
    this.name = "DesktopValidationError";
  }
}
