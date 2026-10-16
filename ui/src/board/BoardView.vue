<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, useId, watch } from "vue";
import type { ComponentPublicInstance, DeepReadonly } from "vue";
import pieceAssetUrl from "@resources/pieces/chess.glb?url";
import { BoardSceneFailure, BoardSceneFailureKind } from "@/board/failure";
import { BoardScene } from "@/board/scene";
import { BoardSemanticGrid } from "@/board/semantic_grid";
import type { DesktopBootstrap } from "@/desktop/generated/contracts";
import "@/board/board.css";

const props = withDefaults(
  defineProps<{
    snapshot: DeepReadonly<DesktopBootstrap> | null;
    disabled?: boolean;
  }>(),
  { disabled: false },
);
const emit = defineEmits<{
  square: [square: number];
  ready: [];
  failure: [message: string];
}>();
const canvas = ref<HTMLCanvasElement | null>(null);
const viewport = ref<HTMLElement | null>(null);
const focused = ref(BoardSemanticGrid.initialFocus);
const failure = ref<string | null>(null);
const ready = ref(false);
const semanticFocused = ref(false);
const descriptionId = useId();
const grid = computed(() => new BoardSemanticGrid(props.snapshot));
const buttons = new Map<number, HTMLButtonElement>();
let scene: BoardScene | null = null;
let resizeObserver: ResizeObserver | null = null;
let disposed = false;

function registerButton(square: number, element: Element | ComponentPublicInstance | null): void {
  if (element instanceof HTMLButtonElement) {
    buttons.set(square, element);
  } else {
    buttons.delete(square);
  }
}

function focusSquare(square: number): void {
  focused.value = square;
  semanticFocused.value = true;
  scene?.focus(square);
}

function blurSquare(): void {
  semanticFocused.value = false;
  scene?.focus(null);
}

function keydown(event: KeyboardEvent, square: number): void {
  const next = grid.value.next(square, event.key);
  if (next !== null) {
    event.preventDefault();
    buttons.get(next)?.focus();
  }
}

function activate(square: number): void {
  if (!props.disabled && props.snapshot && ready.value && !failure.value) {
    emit("square", square);
  }
}

function pick(event: MouseEvent): void {
  const square = scene?.pick(event.clientX, event.clientY);
  if (square !== null && square !== undefined) {
    buttons.get(square)?.focus();
    activate(square);
  }
}

function fail(error: unknown): void {
  if (disposed) {
    return;
  }
  const message =
    error instanceof BoardSceneFailure ? error.message : "The 3D board could not initialize.";
  failure.value = message;
  ready.value = false;
  emit("failure", message);
}

function resize(): void {
  if (viewport.value) {
    scene?.resize(
      viewport.value.clientWidth,
      viewport.value.clientHeight,
      globalThis.devicePixelRatio,
    );
  }
}

onMounted(async () => {
  if (!canvas.value || !viewport.value) {
    fail(
      new BoardSceneFailure(BoardSceneFailureKind.Graphics, "The 3D board surface is unavailable."),
    );
    return;
  }
  const styles = globalThis.getComputedStyle(viewport.value);
  try {
    const initialized = await BoardScene.create({
      canvas: canvas.value,
      assetUrl: pieceAssetUrl,
      colors: {
        light: styles.getPropertyValue("--board-light").trim(),
        dark: styles.getPropertyValue("--board-dark").trim(),
        border: styles.getPropertyValue("--card").trim(),
        focus: styles.getPropertyValue("--ring").trim(),
      },
      failed: fail,
    });
    if (disposed) {
      initialized.dispose();
      return;
    }
    scene = initialized;
    resize();
    scene.update(props.snapshot);
    if (failure.value) {
      return;
    }
    ready.value = true;
    resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(viewport.value);
    emit("ready");
  } catch (error) {
    if (!disposed) {
      fail(error);
    }
  }
});

watch(
  () => props.snapshot,
  (snapshot) => {
    scene?.update(snapshot);
  },
);

onBeforeUnmount(() => {
  disposed = true;
  resizeObserver?.disconnect();
  scene?.dispose();
  buttons.clear();
});
</script>

<template>
  <section class="chess-board" aria-label="3D chess board">
    <div ref="viewport" class="chess-board__viewport">
      <canvas
        ref="canvas"
        class="chess-board__canvas"
        aria-hidden="true"
        data-board-canvas
        @click="pick"
      />
      <div v-if="failure" class="chess-board__notice" role="alert">
        <strong>3D board unavailable</strong>
        <p>{{ failure }}</p>
        <p>Check graphics acceleration and reopen the application.</p>
      </div>
      <p v-else-if="!ready" class="chess-board__notice" role="status">Loading 3D board...</p>
    </div>
    <div
      class="sr-only"
      role="grid"
      aria-label="Chess board, White nearest"
      :aria-describedby="descriptionId"
      :aria-busy="!ready && !failure"
      :aria-disabled="disabled || !ready || !!failure"
      aria-rowcount="8"
      aria-colcount="8"
    >
      <div v-for="(row, rowIndex) in BoardSemanticGrid.rows" :key="rowIndex" role="row">
        <button
          v-for="square in row"
          :key="square"
          :ref="(element) => registerButton(square, element)"
          type="button"
          role="gridcell"
          :data-square="square"
          :aria-label="grid.label(square)"
          :tabindex="square === focused ? 0 : -1"
          @focus="focusSquare(square)"
          @blur="blurSquare"
          @keydown="keydown($event, square)"
          @click="activate(square)"
        >
          {{ grid.label(square) }}
        </button>
      </div>
    </div>
    <p :id="descriptionId" class="chess-board__status" role="status" aria-live="polite">
      {{ semanticFocused ? grid.label(focused) + ". " : "" }}{{ grid.status }}
    </p>
    <p class="chess-board__instructions">
      White nearest. Use arrow keys to explore squares and Enter or Space to choose a square.
    </p>
  </section>
</template>
