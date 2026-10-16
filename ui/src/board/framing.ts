import { MathUtils, PerspectiveCamera, Vector3 } from "three";
import { BoardSemanticGrid } from "@/board/semantic_grid";

export class BoardFraming {
  static readonly initialTiltDegrees = 20;
  static readonly fieldOfViewDegrees = 35;
  static readonly framingMargin = 1.08;
  static readonly initialDistanceMultiplier = 1.1;
  static readonly edge = BoardSemanticGrid.size / 2 + 0.15;
  static readonly base = -0.35;
  static readonly pieceTop = 1.25;
  readonly camera = new PerspectiveCamera(BoardFraming.fieldOfViewDegrees);
  readonly #target = new Vector3(0, (BoardFraming.pieceTop + BoardFraming.base) / 2, 0);

  resize(width: number, height: number): void {
    const aspect = Math.max(width, 1) / Math.max(height, 1);
    const tilt = MathUtils.degToRad(BoardFraming.initialTiltDegrees);
    const direction = new Vector3(0, Math.cos(tilt), Math.sin(tilt));
    const up = new Vector3(0, Math.sin(tilt), -Math.cos(tilt));
    const verticalTangent = Math.tan(MathUtils.degToRad(BoardFraming.fieldOfViewDegrees) / 2);
    const horizontalTangent = verticalTangent * aspect;
    let fitDistance = 0;
    for (const x of [-BoardFraming.edge, BoardFraming.edge]) {
      for (const y of [BoardFraming.base, BoardFraming.pieceTop]) {
        for (const z of [-BoardFraming.edge, BoardFraming.edge]) {
          const offset = new Vector3(x, y, z).sub(this.#target);
          const depth = offset.dot(direction);
          fitDistance = Math.max(
            fitDistance,
            depth + Math.abs(offset.x) / horizontalTangent,
            depth + Math.abs(offset.dot(up)) / verticalTangent,
          );
        }
      }
    }
    const distance =
      fitDistance * BoardFraming.framingMargin * BoardFraming.initialDistanceMultiplier;
    this.camera.aspect = aspect;
    this.camera.near = 0.1;
    this.camera.far = distance * 4;
    this.camera.position.copy(direction.multiplyScalar(distance).add(this.#target));
    this.camera.lookAt(this.#target);
    this.camera.updateProjectionMatrix();
    this.camera.updateMatrixWorld();
  }
}
