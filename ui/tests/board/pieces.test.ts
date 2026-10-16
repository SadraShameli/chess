import { Box3, Color, Mesh, MeshStandardMaterial, Vector3 } from "three";
import type { Object3D } from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import { beforeAll, describe, expect, it } from "vitest";
import assetUrl from "@resources/pieces/chess.glb?url";
import provenance from "@resources/pieces/provenance.json";

class PieceAssetTestError extends Error {}

class PieceAssetFixture {
  #scene: Object3D | undefined;

  async load(): Promise<void> {
    this.#scene = (await new GLTFLoader().loadAsync(assetUrl)).scene;
  }

  node(name: string): Object3D {
    const node = this.#scene?.getObjectByName(name);
    if (!node) {
      throw new PieceAssetTestError(`Missing piece node: ${name}`);
    }
    return node;
  }

  mesh(name: string): Mesh {
    const meshes: Mesh[] = [];
    this.node(name).traverse((node) => {
      if (this.#isMesh(node)) {
        meshes.push(node);
      }
    });
    const mesh = meshes[0];
    if (meshes.length !== 1 || !mesh) {
      throw new PieceAssetTestError(`Expected one sculpted mesh: ${name}`);
    }
    return mesh;
  }

  #isMesh(node: Object3D): node is Mesh {
    return node instanceof Mesh;
  }
}

describe("Bundled original sculpted pieces", () => {
  const fixture = new PieceAssetFixture();
  beforeAll(async () => fixture.load());

  it("provides six distinct silhouettes with two offline satin finishes", () => {
    expect(provenance.pieces).toHaveLength(6);
    expect(new Set(provenance.pieces.map((piece) => piece.kind)).size).toBe(6);
    expect(new Set(provenance.pieces.map((piece) => piece.triangles)).size).toBe(6);
    expect(provenance.pieces.flatMap((piece) => piece.nodes)).toHaveLength(12);
    expect(provenance.construction.externalResources).toBe(0);
    expect(provenance.construction.requiredExtensions).toEqual([]);
  });

  for (const piece of provenance.pieces) {
    it(`loads the ${piece.kind} with a centered base, complete normals and shared geometry`, () => {
      const signatures = [];
      for (const name of piece.nodes) {
        const node = fixture.node(name);
        const mesh = fixture.mesh(name);
        const geometry = mesh.geometry;
        const positions = geometry.getAttribute("position");
        const normals = geometry.getAttribute("normal");
        const indices = geometry.getIndex();
        const bounds = new Box3().setFromObject(node);
        const size = bounds.getSize(new Vector3());
        expect(node.position.toArray()).toEqual([0, 0, 0]);
        expect(bounds.min.y).toBeCloseTo(0, 6);
        expect(bounds.min.x + bounds.max.x).toBeCloseTo(0, 5);
        expect(Math.abs(bounds.min.z + bounds.max.z)).toBeLessThan(0.02);
        expect(size.x).toBeLessThanOrEqual(0.68);
        expect(size.z).toBeLessThanOrEqual(0.68);
        expect(size.y).toBeGreaterThan(0.6);
        expect(size.y).toBeLessThanOrEqual(1.21);
        expect(positions.count).toBe(piece.vertices);
        expect(normals.count).toBe(positions.count);
        expect(indices?.count).toBe(piece.triangles * 3);
        for (let vertex = 0; vertex < positions.count; vertex += 1) {
          expect(Number.isFinite(positions.getX(vertex))).toBe(true);
          expect(Number.isFinite(positions.getY(vertex))).toBe(true);
          expect(Number.isFinite(positions.getZ(vertex))).toBe(true);
          const length = Math.hypot(
            normals.getX(vertex),
            normals.getY(vertex),
            normals.getZ(vertex),
          );
          expect(length).toBeCloseTo(1, 4);
        }
        signatures.push(positions.array);
      }
      expect(signatures[0]).toBe(signatures[1]);
    });
  }

  for (const finish of provenance.materials) {
    it(`loads ${finish.name} as opaque nonmetallic satin without external textures`, () => {
      const mesh = fixture.mesh(`${finish.color}_king`);
      const material = mesh.material;
      if (!(material instanceof MeshStandardMaterial)) {
        throw new PieceAssetTestError(`Expected PBR material: ${finish.name}`);
      }
      expect(material.name).toBe(finish.name);
      expect(material.color.equals(new Color(finish.srgb))).toBe(true);
      expect(material.roughness).toBe(finish.roughness);
      expect(material.metalness).toBe(0);
      expect(material.transparent).toBe(false);
      expect(material.opacity).toBe(1);
      expect(material.map).toBeNull();
      expect(material.normalMap).toBeNull();
    });
  }
});
