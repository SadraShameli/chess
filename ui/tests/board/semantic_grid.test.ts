import { describe, expect, it } from "vitest";
import { BoardSemanticGrid } from "@/board/semantic_grid";
import { BoardTestFixture } from "@tests/board/fixture";

describe("Native snapshot semantic board", () => {
  const fixture = new BoardTestFixture();
  it("covers every square once with White nearest and reads native piece labels", () => {
    const grid = new BoardSemanticGrid(fixture.snapshot);
    const squares = BoardSemanticGrid.rows.flat();
    expect(squares).toHaveLength(64);
    expect(new Set(squares).size).toBe(squares.length);
    expect(squares[0]).toBe(56);
    expect(squares.at(-1)).toBe(7);
    expect(grid.label(0)).toBe("a1, white rook");
    expect(grid.label(60)).toBe("e8, black king");
    expect(grid.label(28)).toBe("e4, empty");
    expect(grid.status).toBe("White to move.");
  });

  it("moves focus in visual orientation without wrapping across edges", () => {
    const grid = new BoardSemanticGrid(null);
    expect(grid.next(0, "ArrowUp")).toBe(8);
    expect(grid.next(8, "ArrowDown")).toBe(0);
    expect(grid.next(0, "ArrowRight")).toBe(1);
    expect(grid.next(7, "ArrowRight")).toBe(7);
    expect(grid.next(0, "ArrowLeft")).toBe(0);
    expect(grid.next(63, "ArrowUp")).toBe(63);
    expect(grid.next(28, "Home")).toBe(24);
    expect(grid.next(28, "End")).toBe(31);
    expect(grid.next(28, "Enter")).toBeNull();
    expect(grid.status).toBe("Waiting for the native position.");
  });

  it("replaces placement and check announcements only from the supplied snapshot", () => {
    const snapshot = fixture.snapshot;
    const grid = new BoardSemanticGrid({
      ...snapshot,
      board: {
        ...snapshot.board,
        turn: "black",
        is_check: true,
        pieces: snapshot.board.pieces.map((piece) =>
          piece.square === 12 ? { ...piece, square: 28 } : piece,
        ),
      },
    });
    expect(grid.label(12)).toBe("e2, empty");
    expect(grid.label(28)).toBe("e4, white pawn");
    expect(grid.status).toBe("Black to move. Check.");
  });
});
