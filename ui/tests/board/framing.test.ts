import { MathUtils, Vector3 } from "three";
import { describe, expect, it } from "vitest";
import { BoardFraming } from "@/board/framing";

describe("Initial 3D board framing", () => {
  it.each([
    [560, 560],
    [1280, 800],
    [300, 800],
  ])("keeps all board and piece bounds inside a %ix%i viewport at 20 degrees", (width, height) => {
    const framing = new BoardFraming();
    framing.resize(width, height);
    expect(framing.camera.fov).toBe(35);
    const viewingDirection = framing.camera.getWorldDirection(new Vector3());
    expect(MathUtils.radToDeg(Math.acos(-viewingDirection.y))).toBeCloseTo(20);
    for (const x of [-BoardFraming.edge, BoardFraming.edge]) {
      for (const y of [BoardFraming.base, BoardFraming.pieceTop]) {
        for (const z of [-BoardFraming.edge, BoardFraming.edge]) {
          const projected = new Vector3(x, y, z).project(framing.camera);
          expect(Math.abs(projected.x)).toBeLessThan(1);
          expect(Math.abs(projected.y)).toBeLessThan(1);
          expect(projected.z).toBeGreaterThan(-1);
          expect(projected.z).toBeLessThan(1);
        }
      }
    }
  });
});
