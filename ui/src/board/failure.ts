export enum BoardSceneFailureKind {
  Graphics = "graphics",
  Assets = "assets",
  Shader = "shader",
  Context = "context",
}

export class BoardSceneFailure extends Error {
  constructor(
    readonly kind: BoardSceneFailureKind,
    message: string,
    cause?: unknown,
  ) {
    super(message, { cause });
    this.name = "BoardSceneFailure";
  }
}
