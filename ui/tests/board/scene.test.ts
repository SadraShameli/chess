import { Vector3 } from "three";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BoardSceneFailure, BoardSceneFailureKind } from "@/board/failure";
import { BoardFraming } from "@/board/framing";
import { BoardScene } from "@/board/scene";
import { BoardSemanticGrid } from "@/board/semantic_grid";
import { BoardTestFixture } from "@tests/board/fixture";

describe("Direct WebGL2 board scene", () => {
  const fixture = new BoardTestFixture();
  const scenes: BoardScene[] = [];

  afterEach(() => {
    for (const scene of scenes.splice(0)) {
      scene.dispose();
    }
    fixture.dispose();
    vi.restoreAllMocks();
  });

  it("loads offline pieces, renders snapshots and picks every board square", async () => {
    const canvas = fixture.canvas();
    const failed = vi.fn();
    const scene = await BoardScene.create(fixture.sceneOptions(canvas, failed));
    scenes.push(scene);
    scene.resize(560, 560, 1);
    scene.update(fixture.snapshot);
    const framing = new BoardFraming();
    framing.resize(560, 560);
    const bounds = canvas.getBoundingClientRect();
    for (let square = 0; square < BoardSemanticGrid.squareCount; square += 1) {
      const center = BoardSemanticGrid.center(square);
      const screen = new Vector3(center.x, 0, center.z).project(framing.camera);
      expect(
        scene.pick(
          bounds.left + ((screen.x + 1) / 2) * bounds.width,
          bounds.top + ((1 - screen.y) / 2) * bounds.height,
        ),
      ).toBe(square);
    }
    expect(failed).not.toHaveBeenCalled();
    expect(scene.pick(bounds.left, bounds.top)).toBeNull();
    const occupiedRendering = canvas.toDataURL();
    scene.update(null);
    expect(canvas.toDataURL()).not.toBe(occupiedRendering);
    scene.focus(28);
    scene.dispose();
    expect(scene.pick(bounds.left + bounds.width / 2, bounds.top + bounds.height / 2)).toBeNull();
  });

  it("fails explicitly when WebGL2 is unavailable", async () => {
    const canvas = fixture.canvas();
    vi.spyOn(canvas, "getContext").mockReturnValue(null);
    await expect(BoardScene.create(fixture.sceneOptions(canvas, vi.fn()))).rejects.toEqual(
      new BoardSceneFailure(
        BoardSceneFailureKind.Graphics,
        "The 3D board requires WebGL2 graphics acceleration.",
      ),
    );
  });

  it("interrupts rendering and picking on context loss", async () => {
    const canvas = fixture.canvas();
    const failed = vi.fn();
    const scene = await BoardScene.create(fixture.sceneOptions(canvas, failed));
    scenes.push(scene);
    scene.resize(560, 560, 1);
    canvas.dispatchEvent(new Event("webglcontextlost", { cancelable: true }));
    expect(failed).toHaveBeenCalledExactlyOnceWith(
      new BoardSceneFailure(
        BoardSceneFailureKind.Context,
        "The 3D board lost its graphics context.",
      ),
    );
    const bounds = canvas.getBoundingClientRect();
    expect(scene.pick(bounds.left + bounds.width / 2, bounds.top + bounds.height / 2)).toBeNull();
  });

  it("rejects corrupt bundled assets instead of substituting another board", async () => {
    const canvas = fixture.canvas();
    const assetUrl = URL.createObjectURL(new Blob(["invalid GLB"], { type: "model/gltf-binary" }));
    try {
      await expect(
        BoardScene.create({ ...fixture.sceneOptions(canvas, vi.fn()), assetUrl }),
      ).rejects.toMatchObject({
        kind: BoardSceneFailureKind.Assets,
        message: "The bundled 3D chess pieces could not load.",
      });
    } finally {
      URL.revokeObjectURL(assetUrl);
    }
  });

  it("reports a missing required piece when applying an authoritative snapshot", async () => {
    const canvas = fixture.canvas();
    const assetUrl = URL.createObjectURL(
      new Blob([JSON.stringify({ asset: { version: "2.0" }, scenes: [{ nodes: [] }], scene: 0 })], {
        type: "model/gltf+json",
      }),
    );
    const failed = vi.fn();
    try {
      const scene = await BoardScene.create({ ...fixture.sceneOptions(canvas, failed), assetUrl });
      scenes.push(scene);
      scene.update(fixture.snapshot);
      expect(failed).toHaveBeenCalledExactlyOnceWith(
        new BoardSceneFailure(BoardSceneFailureKind.Assets, "A required 3D piece is missing."),
      );
    } finally {
      URL.revokeObjectURL(assetUrl);
    }
  });
});
