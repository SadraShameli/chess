import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { Document, NodeIO } from "@gltf-transform/core";
import type { Accessor, Buffer as AssetBuffer, Material, Primitive } from "@gltf-transform/core";
import {
  BoxGeometry,
  Color,
  ExtrudeGeometry,
  LatheGeometry,
  Shape,
  SphereGeometry,
  Vector2,
} from "three";
import type { BufferGeometry } from "three";
import { mergeGeometries, mergeVertices } from "three/addons/utils/BufferGeometryUtils.js";
import { Brush, Evaluator, SUBTRACTION } from "three-bvh-csg/src/index.js";

enum PieceKind {
  Pawn = "pawn",
  Knight = "knight",
  Bishop = "bishop",
  Rook = "rook",
  Queen = "queen",
  King = "king",
}

interface PieceSculpture {
  readonly kind: PieceKind;
  readonly geometry: BufferGeometry;
  readonly silhouette: string;
}

interface PieceFinish {
  readonly color: "white" | "black";
  readonly name: string;
  readonly srgb: string;
  readonly roughness: number;
}

class PieceGenerationError extends Error {}

class PieceSculptureGenerator {
  readonly #directory = new URL("../../resources/pieces/", import.meta.url);
  readonly #radialSegments = 64;
  readonly #curveSegments = 12;
  readonly #bevelSegments = 3;
  readonly #finishes: readonly PieceFinish[] = [
    { color: "white", name: "IvorySatin", srgb: "#f2e6cb", roughness: 0.32 },
    { color: "black", name: "EbonySatin", srgb: "#202328", roughness: 0.36 },
  ];

  async run(): Promise<void> {
    const arguments_ = process.argv.slice(2);
    if (arguments_.length > 1 || (arguments_.length === 1 && arguments_[0] !== "--check")) {
      throw new PieceGenerationError("Usage: generate-pieces.ts [--check]");
    }
    const check = arguments_.length === 1;
    const sculptures: readonly PieceSculpture[] = [
      { kind: PieceKind.Pawn, geometry: this.#pawn(), silhouette: "Round finial and slim neck" },
      {
        kind: PieceKind.Knight,
        geometry: this.#knight(),
        silhouette: "Swept horse profile, ears, muzzle and raised mane",
      },
      {
        kind: PieceKind.Bishop,
        geometry: this.#bishop(),
        silhouette: "Pointed mitre with a carved diagonal slit",
      },
      {
        kind: PieceKind.Rook,
        geometry: this.#rook(),
        silhouette: "Broad tower with six open crenellations",
      },
      {
        kind: PieceKind.Queen,
        geometry: this.#queen(),
        silhouette: "Eight rising crown points with pearl tips",
      },
      {
        kind: PieceKind.King,
        geometry: this.#king(),
        silhouette: "Tall stem and beveled cross finial",
      },
    ];
    const document = new Document();
    document.getRoot().getAsset().generator =
      "Reckless original piece sculptures; glTF-Transform 4.5.1";
    document.getRoot().getAsset().copyright =
      "Original project-owned parametric artwork. Private personal use.";
    const scene = document.createScene("ChessPieceLibrary");
    const buffer = document.createBuffer("SharedPieceGeometry");
    const finishes = this.#finishes.map((finish) => ({
      finish,
      material: this.#material(document, finish),
    }));
    const pieces = sculptures.map((sculpture) => {
      const primitive = this.#primitive(document, buffer, sculpture);
      const nodes = finishes.map(({ finish, material }) => {
        const name = `${finish.color}_${sculpture.kind}`;
        const mesh = document
          .createMesh(`${name}_mesh`)
          .addPrimitive(primitive.clone().setMaterial(material));
        const node = document
          .createNode(name)
          .addChild(document.createNode(`${name}_sculpture`).setMesh(mesh));
        scene.addChild(node);
        return name;
      });
      sculpture.geometry.computeBoundingBox();
      const bounds = sculpture.geometry.boundingBox;
      const position = sculpture.geometry.getAttribute("position");
      const indices = sculpture.geometry.getIndex();
      if (!bounds || !indices) {
        throw new PieceGenerationError(
          `Missing generated geometry bounds or indices: ${sculpture.kind}`,
        );
      }
      return {
        kind: sculpture.kind,
        silhouette: sculpture.silhouette,
        nodes,
        vertices: position.count,
        triangles: indices.count / 3,
        bounds: { min: bounds.min.toArray(), max: bounds.max.toArray() },
      };
    });
    const binary = await new NodeIO().writeBinary(document);
    const source = await readFile(new URL(import.meta.url));
    const provenance = `${JSON.stringify(
      {
        schemaVersion: 1,
        title: "Reckless sculpted chess pieces",
        authorship:
          "Original parametric artwork authored for this project. No downloaded models, textures or paid assets.",
        rights:
          "Project owner retains rights. Private personal use; no separate application or artwork license is granted.",
        generator: {
          path: "ui/tools/generate-pieces.ts",
          sha256: this.#sha256(source),
          command: "cd ui && bun run pieces:generate",
        },
        asset: {
          path: "resources/pieces/chess.glb",
          sha256: this.#sha256(binary),
          bytes: binary.byteLength,
          format: "glTF 2.0 binary",
        },
        coordinates: {
          up: "+Y",
          knightForward: "-Z",
          squareSize: 1,
          origin: "Base center at x=0,z=0; bottom at y=0",
          placement: "Clone a named group; all library nodes overlap at the origin by design",
        },
        construction: {
          radialSegments: this.#radialSegments,
          curveSegments: this.#curveSegments,
          bevelSegments: this.#bevelSegments,
          geometrySharing: "Six indexed geometries shared by twelve color variants",
          externalResources: 0,
          requiredExtensions: [],
          metallicFactor: 0,
        },
        materials: this.#finishes,
        pieces,
        dependencies: [
          {
            name: "three",
            version: "0.186.1",
            license: "MIT",
            role: "Parametric geometry and linear color conversion",
          },
          {
            name: "three-bvh-csg",
            version: "0.0.18",
            license: "MIT",
            role: "Bishop mitre slit subtraction",
          },
          {
            name: "three-mesh-bvh",
            version: "0.9.7",
            license: "MIT",
            role: "Pinned compatible CSG spatial index",
          },
          {
            name: "@gltf-transform/core",
            version: "4.5.1",
            license: "MIT",
            role: "Standard GLB serialization",
          },
        ],
        visualAcceptance:
          "Six silhouettes and materials are generated and browser-tested. Premium readability across supported views and native WebViews remains a visual acceptance gate.",
      },
      null,
      2,
    )}\n`;
    if (!check) {
      await mkdir(this.#directory, { recursive: true });
    }
    await this.#output("chess.glb", binary, check);
    await this.#output("provenance.json", new TextEncoder().encode(provenance), check);
    for (const sculpture of sculptures) {
      sculpture.geometry.dispose();
    }
  }

  #material(document: Document, finish: PieceFinish): Material {
    const color = new Color(finish.srgb);
    return document
      .createMaterial(finish.name)
      .setBaseColorFactor([color.r, color.g, color.b, 1])
      .setMetallicFactor(0)
      .setRoughnessFactor(finish.roughness);
  }

  #primitive(document: Document, buffer: AssetBuffer, sculpture: PieceSculpture): Primitive {
    const geometry = sculpture.geometry;
    const position = geometry.getAttribute("position");
    const normal = geometry.getAttribute("normal");
    const index = geometry.getIndex();
    if (!index || position.itemSize !== 3 || normal.itemSize !== 3) {
      throw new PieceGenerationError(`Invalid generated geometry: ${sculpture.kind}`);
    }
    const accessor = (name: string, values: Float32Array<ArrayBuffer>): Accessor =>
      document.createAccessor(`${sculpture.kind}_${name}`, buffer).setType("VEC3").setArray(values);
    const positions = new Float32Array(position.count * 3);
    const normals = new Float32Array(normal.count * 3);
    for (let vertex = 0; vertex < position.count; vertex += 1) {
      const offset = vertex * 3;
      positions.set([position.getX(vertex), position.getY(vertex), position.getZ(vertex)], offset);
      normals.set([normal.getX(vertex), normal.getY(vertex), normal.getZ(vertex)], offset);
    }
    const indices = new Uint32Array(index.count);
    for (let entry = 0; entry < index.count; entry += 1) {
      indices[entry] = index.getX(entry);
    }
    return document
      .createPrimitive()
      .setAttribute("POSITION", accessor("position", positions))
      .setAttribute("NORMAL", accessor("normal", normals))
      .setIndices(
        document
          .createAccessor(`${sculpture.kind}_indices`, buffer)
          .setType("SCALAR")
          .setArray(indices),
      );
  }

  #lathe(profile: readonly (readonly [number, number])[]): LatheGeometry {
    return new LatheGeometry(
      profile.map(([radius, height]) => new Vector2(radius, height)),
      this.#radialSegments,
    );
  }

  #base(radius: number): LatheGeometry {
    return this.#lathe([
      [0, 0],
      [radius * 0.83, 0],
      [radius * 0.94, 0.012],
      [radius, 0.034],
      [radius, 0.055],
      [radius * 0.98, 0.075],
      [radius * 0.91, 0.093],
      [radius * 0.84, 0.105],
      [radius * 0.83, 0.118],
      [radius * 0.87, 0.129],
      [radius * 0.87, 0.145],
      [radius * 0.81, 0.16],
      [radius * 0.66, 0.172],
      [0, 0.172],
    ]);
  }

  #sphere(radius: number, x: number, y: number, z: number): SphereGeometry {
    return new SphereGeometry(radius, 32, 20).translate(x, y, z);
  }

  #ellipsoid(
    size: readonly [number, number, number],
    position: readonly [number, number, number],
  ): SphereGeometry {
    return new SphereGeometry(1, 32, 20).scale(...size).translate(...position);
  }

  #extrude(shape: Shape, depth: number, bevel: number, bevelThickness = bevel): ExtrudeGeometry {
    return new ExtrudeGeometry(shape, {
      depth,
      steps: 1,
      bevelEnabled: true,
      bevelSegments: this.#bevelSegments,
      bevelSize: bevel,
      bevelThickness,
      curveSegments: this.#curveSegments,
    }).translate(0, 0, -depth / 2);
  }

  #combine(parts: readonly BufferGeometry[]): BufferGeometry {
    const flattened = parts.map((part) => {
      const geometry = part.index ? part.toNonIndexed() : part.clone();
      geometry.deleteAttribute("uv");
      geometry.clearGroups();
      return geometry;
    });
    const merged = mergeGeometries(flattened);
    const indexed = mergeVertices(merged);
    indexed.normalizeNormals();
    merged.dispose();
    for (const geometry of [...flattened, ...parts]) {
      geometry.dispose();
    }
    return indexed;
  }

  #pawn(): BufferGeometry {
    return this.#combine([
      this.#base(0.255),
      this.#lathe([
        [0, 0.153],
        [0.17, 0.153],
        [0.16, 0.19],
        [0.126, 0.235],
        [0.095, 0.3],
        [0.085, 0.365],
        [0.104, 0.394],
        [0.145, 0.406],
        [0.15, 0.425],
        [0.132, 0.445],
        [0, 0.452],
      ]),
      this.#sphere(0.124, 0, 0.549, 0),
    ]);
  }

  #knight(): BufferGeometry {
    const horse = new Shape();
    horse.moveTo(-0.18, 0.25);
    horse.bezierCurveTo(-0.265, 0.48, -0.21, 0.75, -0.13, 0.855);
    horse.quadraticCurveTo(-0.035, 0.925, 0.065, 0.855);
    horse.bezierCurveTo(0.15, 0.82, 0.19, 0.74, 0.25, 0.705);
    horse.lineTo(0.272, 0.625);
    horse.quadraticCurveTo(0.25, 0.582, 0.206, 0.577);
    horse.lineTo(0.085, 0.615);
    horse.bezierCurveTo(0.025, 0.535, -0.04, 0.41, 0.09, 0.26);
    horse.closePath();
    const mane = new Shape();
    mane.moveTo(-0.18, 0.31);
    mane.bezierCurveTo(-0.32, 0.52, -0.25, 0.755, -0.145, 0.878);
    mane.lineTo(-0.12, 0.81);
    mane.bezierCurveTo(-0.205, 0.66, -0.2, 0.48, -0.13, 0.35);
    mane.closePath();
    return this.#combine([
      this.#base(0.29),
      this.#lathe([
        [0, 0.15],
        [0.2, 0.15],
        [0.2, 0.19],
        [0.178, 0.218],
        [0.16, 0.24],
        [0, 0.24],
      ]),
      this.#extrude(horse, 0.025, 0.03, 0.12).rotateY(Math.PI / 2),
      this.#extrude(mane, 0.055, 0.014).rotateY(Math.PI / 2),
      this.#ellipsoid([0.137, 0.128, 0.136], [0, 0.777, -0.07]),
      this.#ellipsoid([0.129, 0.07, 0.114], [0, 0.645, -0.176]),
      this.#ear(-0.077),
      this.#ear(0.077),
      this.#sphere(0.017, -0.137, 0.785, -0.109),
      this.#sphere(0.017, 0.137, 0.785, -0.109),
    ]);
  }

  #ear(x: number): ExtrudeGeometry {
    const ear = new Shape();
    ear.moveTo(-0.034, 0);
    ear.lineTo(-0.014, 0.117);
    ear.lineTo(0.03, 0.011);
    ear.closePath();
    return this.#extrude(ear, 0.045, 0.009).translate(x, 0.861, -0.023);
  }

  #bishop(): BufferGeometry {
    const head = new Brush(
      this.#lathe([
        [0, 0.653],
        [0.065, 0.66],
        [0.116, 0.7],
        [0.147, 0.75],
        [0.15, 0.79],
        [0.132, 0.84],
        [0.094, 0.895],
        [0.045, 0.95],
        [0, 0.99],
      ]),
    );
    const slit = new Brush(new BoxGeometry(0.045, 0.34, 0.42));
    slit.rotation.z = -Math.PI / 5;
    slit.position.set(0.09, 0.91, 0);
    head.updateMatrixWorld(true);
    slit.updateMatrixWorld(true);
    const evaluator = new Evaluator();
    evaluator.useGroups = false;
    const mitre = evaluator.evaluate(head, slit, SUBTRACTION);
    const geometry = this.#combine([
      this.#base(0.285),
      this.#lathe([
        [0, 0.15],
        [0.195, 0.15],
        [0.175, 0.205],
        [0.135, 0.26],
        [0.101, 0.37],
        [0.085, 0.48],
        [0.097, 0.555],
        [0.145, 0.583],
        [0.155, 0.605],
        [0.145, 0.63],
        [0.095, 0.649],
        [0, 0.66],
      ]),
      mitre.geometry,
    ]);
    head.geometry.dispose();
    slit.geometry.dispose();
    head.disposeCacheData();
    slit.disposeCacheData();
    return geometry;
  }

  #rook(): BufferGeometry {
    const parts: BufferGeometry[] = [
      this.#base(0.3),
      this.#lathe([
        [0, 0.15],
        [0.205, 0.15],
        [0.2, 0.205],
        [0.17, 0.24],
        [0.15, 0.34],
        [0.155, 0.52],
        [0.18, 0.59],
        [0.237, 0.615],
        [0.249, 0.638],
        [0.249, 0.67],
        [0.23, 0.69],
        [0, 0.69],
      ]),
    ];
    const teeth = 6;
    for (let tooth = 0; tooth < teeth; tooth += 1) {
      const start = (tooth * Math.PI * 2) / teeth;
      const end = start + (Math.PI / teeth) * 0.98;
      const crown = new Shape();
      crown.absarc(0, 0, 0.234, start, end, false);
      crown.lineTo(Math.cos(end) * 0.156, Math.sin(end) * 0.156);
      crown.absarc(0, 0, 0.156, end, start, true);
      crown.closePath();
      parts.push(
        this.#extrude(crown, 0.119, 0.009)
          .rotateX(-Math.PI / 2)
          .translate(0, 0.735, 0),
      );
    }
    return this.#combine(parts);
  }

  #queen(): BufferGeometry {
    const parts: BufferGeometry[] = [
      this.#base(0.318),
      this.#lathe([
        [0, 0.15],
        [0.215, 0.15],
        [0.195, 0.21],
        [0.147, 0.28],
        [0.106, 0.4],
        [0.088, 0.56],
        [0.107, 0.67],
        [0.165, 0.71],
        [0.176, 0.734],
        [0.17, 0.76],
        [0.13, 0.776],
        [0.14, 0.82],
        [0.2, 0.897],
        [0.21, 0.92],
        [0.195, 0.94],
        [0, 0.94],
      ]),
      this.#sphere(0.055, 0, 0.941, 0),
    ];
    const points = 8;
    for (let point = 0; point < points; point += 1) {
      const angle = (point * Math.PI * 2) / points;
      const x = Math.sin(angle) * 0.185;
      const z = Math.cos(angle) * 0.185;
      parts.push(
        this.#lathe([
          [0, 0.883],
          [0.044, 0.885],
          [0.039, 0.935],
          [0.025, 0.995],
          [0, 1.027],
        ]).translate(x, 0, z),
      );
      parts.push(this.#sphere(0.031, x, 1.022, z));
    }
    return this.#combine(parts);
  }

  #king(): BufferGeometry {
    const cross = new Shape();
    cross.moveTo(-0.032, 0.944);
    cross.lineTo(0.032, 0.944);
    cross.lineTo(0.032, 1.056);
    cross.lineTo(0.109, 1.056);
    cross.lineTo(0.109, 1.116);
    cross.lineTo(0.032, 1.116);
    cross.lineTo(0.032, 1.19);
    cross.lineTo(-0.032, 1.19);
    cross.lineTo(-0.032, 1.116);
    cross.lineTo(-0.109, 1.116);
    cross.lineTo(-0.109, 1.056);
    cross.lineTo(-0.032, 1.056);
    cross.closePath();
    return this.#combine([
      this.#base(0.33),
      this.#lathe([
        [0, 0.15],
        [0.222, 0.15],
        [0.205, 0.21],
        [0.16, 0.28],
        [0.113, 0.43],
        [0.096, 0.61],
        [0.115, 0.724],
        [0.18, 0.755],
        [0.19, 0.778],
        [0.18, 0.802],
        [0.127, 0.82],
        [0.12, 0.863],
        [0.16, 0.897],
        [0.164, 0.919],
        [0.14, 0.943],
        [0, 0.955],
      ]),
      this.#extrude(cross, 0.064, 0.009),
    ]);
  }

  #sha256(content: Uint8Array): string {
    return createHash("sha256").update(content).digest("hex");
  }

  async #output(name: string, content: Uint8Array, check: boolean): Promise<void> {
    const path = new URL(name, this.#directory);
    if (check) {
      const existing = await readFile(path);
      if (!existing.equals(content)) {
        throw new PieceGenerationError(
          `Generated chess piece drift: ${name}; run bun run pieces:generate`,
        );
      }
    } else {
      await writeFile(path, content);
    }
  }
}

await new PieceSculptureGenerator().run();
