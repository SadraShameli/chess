import type { DeepReadonly } from "vue";
import assetUrl from "@resources/pieces/chess.glb?url";
import fixture from "@/desktop/generated/fixture.json";
import type { DesktopBootstrap } from "@/desktop/generated/contracts";
import { DesktopValidation } from "@/desktop/validation";
import type { BoardSceneOptions } from "@/board/scene";
import type { BoardSceneFailure } from "@/board/failure";

export class BoardTestFixture {
  readonly snapshot: DeepReadonly<DesktopBootstrap>;
  readonly #canvases: HTMLCanvasElement[] = [];

  constructor() {
    const response = new DesktopValidation().response(fixture.response);
    if (response.result.status !== "success" || response.result.value.operation !== "bootstrap") {
      throw new Error("Missing native bootstrap fixture");
    }
    this.snapshot = response.result.value.value;
  }

  canvas(): HTMLCanvasElement {
    const canvas = document.createElement("canvas");
    canvas.style.width = "560px";
    canvas.style.height = "560px";
    document.body.append(canvas);
    this.#canvases.push(canvas);
    return canvas;
  }

  sceneOptions(
    canvas: HTMLCanvasElement,
    failed: (failure: BoardSceneFailure) => void,
  ): BoardSceneOptions {
    return {
      canvas,
      assetUrl,
      colors: { light: "#eee5cf", dark: "#4b7057", border: "#1b222b", focus: "#34d399" },
      failed,
    };
  }

  dispose(): void {
    for (const canvas of this.#canvases.splice(0)) {
      canvas.remove();
    }
  }
}
