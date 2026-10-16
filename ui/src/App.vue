<script setup lang="ts">
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, shallowRef } from "vue";
import { DesktopApplicationEnumPhase } from "@/desktop/application";
import type { DesktopApplication } from "@/desktop/application";
import PlayerBar from "@/shell/PlayerBar.vue";

const BoardView = defineAsyncComponent(() => import("@/board/BoardView.vue"));
const props = defineProps<{ application: DesktopApplication }>();
const state = props.application.state;
const graphicsFailure = shallowRef<string | null>(null);
const engine = computed(() => state.value.snapshot?.engine);
const engineName = computed(() =>
  engine.value?.state === "ready" ? engine.value.identity.name : "Reckless unavailable",
);

onMounted(async () => {
  await props.application.start();
});
onBeforeUnmount(() => {
  props.application.dispose();
});

function graphicsFailed(message: string): void {
  graphicsFailure.value = message;
}
</script>

<template>
  <main class="flex h-dvh min-h-0 flex-col overflow-hidden bg-background text-foreground">
    <header class="flex shrink-0 items-center justify-between border-b px-7 py-5">
      <div class="flex items-center gap-3">
        <span
          class="flex size-10 items-center justify-center rounded-xl bg-primary text-xl text-primary-foreground"
          aria-hidden="true"
          >R</span
        >
        <div>
          <h1 class="text-lg font-semibold tracking-tight">Reckless Chess</h1>
          <p class="text-xs text-muted-foreground">Your offline chess space</p>
        </div>
      </div>
      <span class="rounded-full border bg-card px-3 py-1 text-xs text-muted-foreground"
        >Offline</span
      >
    </header>
    <div class="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_19rem] gap-6 p-6">
      <section
        aria-label="Chess board"
        class="mx-auto grid min-h-0 w-full grid-rows-[auto_minmax(0,1fr)_auto] gap-3"
      >
        <PlayerBar name="Black" side="B" />
        <div class="min-h-0 min-w-0">
          <BoardView :snapshot="state.snapshot" disabled @failure="graphicsFailed" />
        </div>
        <PlayerBar name="White" side="W" />
      </section>
      <aside class="min-h-0 space-y-5 overflow-y-auto" aria-label="Position and engine">
        <section class="rounded-xl border bg-card p-5" aria-labelledby="position-heading">
          <p class="mb-2 text-xs font-medium tracking-wider text-primary uppercase">
            Board overview
          </p>
          <h2 id="position-heading" class="text-xl font-semibold">Starting position</h2>
          <p class="mt-2 text-sm leading-relaxed text-muted-foreground">
            Standard chess. White to move.
          </p>
          <p class="mt-4 border-t pt-4 text-sm text-muted-foreground">No game has started.</p>
          <button
            class="mt-5 w-full rounded-lg border px-4 py-2 text-sm font-medium hover:bg-accent disabled:opacity-50"
            :disabled="state.phase === DesktopApplicationEnumPhase.Loading"
            @click="application.refresh()"
          >
            Refresh position
          </button>
        </section>
        <section class="rounded-xl border bg-card p-5" aria-labelledby="engine-heading">
          <h2 id="engine-heading" class="text-sm font-semibold">{{ engineName }}</h2>
          <template v-if="engine?.state === 'ready'">
            <p class="mt-2 text-sm text-primary" role="status">Engine ready</p>
            <p class="mt-2 text-xs text-muted-foreground">{{ engine.identity.author }}</p>
            <p class="mt-4 text-xs text-muted-foreground">
              {{ engine.identity.supported_options.length }} verified engine options
            </p>
          </template>
          <p
            v-else-if="engine?.state === 'unavailable'"
            class="mt-2 text-sm leading-relaxed text-muted-foreground"
            role="alert"
          >
            {{ engine.message }}
          </p>
          <p v-else class="mt-2 text-sm text-muted-foreground" role="status">
            Connecting to the native engine...
          </p>
          <p v-if="state.snapshot" class="mt-3 text-xs text-muted-foreground">
            {{ state.snapshot.cpu_threads }} CPU threads available
          </p>
        </section>
        <p
          v-if="state.phase === DesktopApplicationEnumPhase.Loading"
          role="status"
          class="px-1 text-sm text-muted-foreground"
        >
          Loading native position...
        </p>
        <div
          v-if="state.message"
          role="alert"
          class="rounded-xl border border-destructive p-4 text-sm"
        >
          <p>{{ state.message }}</p>
          <button class="mt-3 rounded-lg border px-3 py-2 font-medium" @click="application.start()">
            Retry connection
          </button>
        </div>
        <p
          v-if="graphicsFailure"
          role="alert"
          class="rounded-xl border border-destructive p-4 text-sm"
        >
          {{ graphicsFailure }}
        </p>
      </aside>
    </div>
  </main>
</template>
