<script setup lang="ts">
defineProps<{
  name: string;
  side: string;
}>();
</script>

<template>
  <div class="flex items-center justify-between rounded-xl border bg-card px-5 py-3">
    <div class="flex items-center gap-3">
      <span
        class="flex size-10 items-center justify-center rounded-lg bg-muted text-lg"
        aria-hidden="true"
        >{{ side }}</span
      >
      <div>
        <p class="font-semibold">{{ name }}</p>
        <p class="text-xs text-muted-foreground">No game started</p>
      </div>
    </div>
    <span class="rounded-lg bg-muted px-4 py-2 text-sm text-muted-foreground">Untimed</span>
  </div>
</template>
