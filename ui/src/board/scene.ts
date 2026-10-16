import {
  AmbientLight,
  BoxGeometry,
  BufferGeometry,
  Color,
  DirectionalLight,
  EdgesGeometry,
  Group,
  LineBasicMaterial,
  LineSegments,
  Material,
  Mesh,
  MeshStandardMaterial,
  Raycaster,
  Scene,
  Vector2,
  WebGLRenderer,
} from "three";
import type { Object3D } from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import type { DeepReadonly } from "vue";
import { BoardSceneFailure, BoardSceneFailureKind } from "@/board/failure";
import { BoardFraming } from "@/board/framing";
import { BoardSemanticGrid } from "@/board/semantic_grid";
import type { DesktopBootstrap } from "@/desktop/generated/contracts";

export interface BoardSceneOptions {
  readonly canvas: HTMLCanvasElement;
  readonly assetUrl: string;
  readonly colors: {
    readonly light: string;
    readonly dark: string;
    readonly border: string;
    readonly focus: string;
  };
  readonly failed: (failure: BoardSceneFailure) => void;
}

interface BoardSceneResource {
  dispose(): void;
}

export class BoardScene {
  static readonly #pixelRatioLimit = 1.5;
  readonly #scene = new Scene();
  readonly #framing = new BoardFraming();
  readonly #pieces = new Group();
  readonly #raycaster = new Raycaster();
  readonly #pointer = new Vector2();
  readonly #squareByRoot = new Map<Object3D, number>();
  readonly #resources = new Set<BoardSceneResource>();
  readonly #focus: LineSegments<EdgesGeometry, LineBasicMaterial>;
  #templates: Object3D | null = null;
  #failure: BoardSceneFailure | null = null;
  #disposed = false;

  private constructor(
    readonly options: BoardSceneOptions,
    readonly renderer: WebGLRenderer,
  ) {
    const tileGeometry = new BoxGeometry(1, 0.12, 1);
    const light = new MeshStandardMaterial({
      color: new Color(options.colors.light),
      roughness: 0.8,
    });
    const dark = new MeshStandardMaterial({
      color: new Color(options.colors.dark),
      roughness: 0.8,
    });
    for (let square = 0; square < BoardSemanticGrid.squareCount; square += 1) {
      const file = square % BoardSemanticGrid.size;
      const rank = Math.floor(square / BoardSemanticGrid.size);
      const tile = new Mesh(tileGeometry, (file + rank) % 2 === 0 ? dark : light);
      const center = BoardSemanticGrid.center(square);
      tile.position.set(center.x, -0.06, center.z);
      this.#scene.add(tile);
      this.#squareByRoot.set(tile, square);
    }
    const border = new Mesh(
      new BoxGeometry(BoardFraming.edge * 2, 0.23, BoardFraming.edge * 2),
      new MeshStandardMaterial({ color: new Color(options.colors.border), roughness: 0.65 }),
    );
    border.position.y = -0.235;
    this.#focus = new LineSegments(
      new EdgesGeometry(tileGeometry),
      new LineBasicMaterial({ color: new Color(options.colors.focus), depthTest: false }),
    );
    this.#focus.visible = false;
    this.#focus.renderOrder = 1;
    const key = new DirectionalLight(0xfff4dc, 3);
    key.position.set(-3, 8, 5);
    const fill = new DirectionalLight(0xd5e7ff, 1.5);
    fill.position.set(4, 5, -3);
    this.#scene.add(border, this.#pieces, this.#focus, key, fill, new AmbientLight(0xffffff, 1.3));
    this.#trackResources(this.#scene);
    renderer.setClearColor(0x000000, 0);
    renderer.debug.onShaderError = () => {
      this.#interrupt(
        new BoardSceneFailure(
          BoardSceneFailureKind.Shader,
          "The 3D board shader could not compile.",
        ),
      );
    };
    options.canvas.addEventListener("webglcontextlost", this.#contextLost);
  }

  static async create(options: BoardSceneOptions): Promise<BoardScene> {
    const context = options.canvas.getContext("webgl2", { alpha: true, antialias: true });
    if (!context) {
      throw new BoardSceneFailure(
        BoardSceneFailureKind.Graphics,
        "The 3D board requires WebGL2 graphics acceleration.",
      );
    }
    let renderer: WebGLRenderer;
    try {
      renderer = new WebGLRenderer({
        canvas: options.canvas,
        context,
        alpha: true,
        antialias: true,
      });
    } catch (cause) {
      throw new BoardSceneFailure(
        BoardSceneFailureKind.Graphics,
        "The 3D board could not initialize graphics.",
        cause,
      );
    }
    const scene = new BoardScene(options, renderer);
    try {
      await scene.#loadAssets();
      return scene;
    } catch (cause) {
      scene.dispose();
      throw cause;
    }
  }

  resize(width: number, height: number, pixelRatio: number): void {
    if (this.#disposed || this.#failure) {
      return;
    }
    this.#framing.resize(width, height);
    this.renderer.setPixelRatio(Math.min(Math.max(pixelRatio, 1), BoardScene.#pixelRatioLimit));
    this.renderer.setSize(Math.max(width, 1), Math.max(height, 1), false);
    this.#render();
  }

  update(snapshot: DeepReadonly<DesktopBootstrap> | null): void {
    if (this.#disposed || this.#failure) {
      return;
    }
    for (const piece of this.#pieces.children) {
      this.#squareByRoot.delete(piece);
    }
    this.#pieces.clear();
    for (const piece of snapshot?.board.pieces ?? []) {
      const template = this.#templates?.getObjectByName(piece.color + "_" + piece.kind);
      if (!template) {
        this.#interrupt(
          new BoardSceneFailure(BoardSceneFailureKind.Assets, "A required 3D piece is missing."),
        );
        return;
      }
      const rendered = template.clone();
      const center = BoardSemanticGrid.center(piece.square);
      rendered.position.set(center.x, 0, center.z);
      rendered.rotation.y = piece.color === "black" ? Math.PI : 0;
      this.#pieces.add(rendered);
      this.#squareByRoot.set(rendered, piece.square);
    }
    this.#render();
  }

  focus(square: number | null): void {
    this.#focus.visible = square !== null;
    if (square !== null) {
      const center = BoardSemanticGrid.center(square);
      this.#focus.position.set(center.x, 0.01, center.z);
    }
    this.#render();
  }

  pick(clientX: number, clientY: number): number | null {
    if (this.#disposed || this.#failure) {
      return null;
    }
    const bounds = this.options.canvas.getBoundingClientRect();
    if (bounds.width === 0 || bounds.height === 0) {
      return null;
    }
    this.#pointer.set(
      ((clientX - bounds.left) / bounds.width) * 2 - 1,
      1 - ((clientY - bounds.top) / bounds.height) * 2,
    );
    this.#raycaster.setFromCamera(this.#pointer, this.#framing.camera);
    const hits = this.#raycaster.intersectObjects([...this.#squareByRoot.keys()], true);
    for (const hit of hits) {
      let object: Object3D | null = hit.object;
      while (object) {
        const square = this.#squareByRoot.get(object);
        if (square !== undefined) {
          return square;
        }
        object = object.parent;
      }
    }
    return null;
  }

  dispose(): void {
    if (this.#disposed) {
      return;
    }
    this.#disposed = true;
    this.options.canvas.removeEventListener("webglcontextlost", this.#contextLost);
    for (const resource of this.#resources) {
      resource.dispose();
    }
    this.#resources.clear();
    this.#squareByRoot.clear();
    this.#pieces.clear();
    this.#scene.clear();
    this.#templates = null;
    this.renderer.dispose();
  }

  async #loadAssets(): Promise<void> {
    try {
      const gltf = await new GLTFLoader().loadAsync(this.options.assetUrl);
      this.#templates = gltf.scene;
      this.#trackResources(gltf.scene);
      await this.renderer.compileAsync(gltf.scene, this.#framing.camera, this.#scene);
    } catch (cause) {
      throw new BoardSceneFailure(
        BoardSceneFailureKind.Assets,
        "The bundled 3D chess pieces could not load.",
        cause,
      );
    }
    if (this.#failure) {
      throw this.#failure;
    }
  }

  #trackResources(root: Object3D): void {
    root.traverse((object) => {
      if (object instanceof Mesh || object instanceof LineSegments) {
        const geometry: unknown = object.geometry;
        if (geometry instanceof BufferGeometry) {
          this.#resources.add(geometry);
        }
        this.#trackMaterial(object.material);
      }
    });
  }

  #trackMaterial(material: unknown): void {
    if (material instanceof Material) {
      this.#resources.add(material);
    } else if (Array.isArray(material)) {
      const materials: readonly unknown[] = material;
      for (const entry of materials) {
        this.#trackMaterial(entry);
      }
    }
  }

  #render(): void {
    if (this.#disposed || this.#failure) {
      return;
    }
    try {
      this.renderer.render(this.#scene, this.#framing.camera);
    } catch (cause) {
      this.#interrupt(
        new BoardSceneFailure(
          BoardSceneFailureKind.Graphics,
          "The 3D board could not render.",
          cause,
        ),
      );
    }
  }

  #interrupt(failure: BoardSceneFailure): void {
    if (!this.#failure && !this.#disposed) {
      this.#failure = failure;
      this.options.failed(failure);
    }
  }

  readonly #contextLost = (event: Event): void => {
    event.preventDefault();
    this.#interrupt(
      new BoardSceneFailure(
        BoardSceneFailureKind.Context,
        "The 3D board lost its graphics context.",
      ),
    );
  };
}
