import type { DeepReadonly } from "vue";
import type { DesktopBootstrap } from "@/desktop/generated/contracts";

const BoardGridKey = Object.freeze({
  Left: "ArrowLeft",
  Right: "ArrowRight",
  Up: "ArrowUp",
  Down: "ArrowDown",
  Start: "Home",
  End: "End",
});

export class BoardSemanticGrid {
  static readonly size = 8;
  static readonly squareCount = BoardSemanticGrid.size ** 2;
  static readonly initialFocus = 0;
  static readonly rows: readonly (readonly number[])[] = Array.from(
    { length: BoardSemanticGrid.size },
    (_, row) =>
      Array.from(
        { length: BoardSemanticGrid.size },
        (_, file) => (BoardSemanticGrid.size - row - 1) * BoardSemanticGrid.size + file,
      ),
  );

  constructor(readonly snapshot: DeepReadonly<DesktopBootstrap> | null) {}

  static coordinate(square: number): string {
    const file = String.fromCharCode("a".charCodeAt(0) + (square % this.size));
    return file + String(Math.floor(square / this.size) + 1);
  }

  static center(square: number): { x: number; z: number } {
    const offset = (this.size - 1) / 2;
    return {
      x: (square % this.size) - offset,
      z: offset - Math.floor(square / this.size),
    };
  }

  label(square: number): string {
    const piece = this.snapshot?.board.pieces.find((piece) => piece.square === square);
    const placement = piece ? piece.color + " " + piece.kind : "empty";
    return BoardSemanticGrid.coordinate(square) + ", " + placement;
  }

  get status(): string {
    if (!this.snapshot) {
      return "Waiting for the native position.";
    }
    const turn = this.snapshot.board.turn === "white" ? "White" : "Black";
    return turn + " to move." + (this.snapshot.board.is_check ? " Check." : "");
  }

  next(square: number, key: string): number | null {
    const file = square % BoardSemanticGrid.size;
    const rank = Math.floor(square / BoardSemanticGrid.size);
    switch (key) {
      case BoardGridKey.Left:
        return square - Math.min(file, 1);
      case BoardGridKey.Right:
        return square + Math.min(BoardSemanticGrid.size - file - 1, 1);
      case BoardGridKey.Up:
        return square + Math.min(BoardSemanticGrid.size - rank - 1, 1) * BoardSemanticGrid.size;
      case BoardGridKey.Down:
        return square - Math.min(rank, 1) * BoardSemanticGrid.size;
      case BoardGridKey.Start:
        return square - file;
      case BoardGridKey.End:
        return square + BoardSemanticGrid.size - file - 1;
      default:
        return null;
    }
  }
}
