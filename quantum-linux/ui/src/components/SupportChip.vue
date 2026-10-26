<script setup lang="ts">
// How well a feature works on a model, from QuantumEngine's catalogue.

import type { FeatureDto } from "../bindings";

defineProps<{ feature: Pick<FeatureDto, "status" | "statusLabel"> }>();
</script>

<template>
  <span class="chip" :class="feature.status">{{ feature.statusLabel }}</span>
</template>

<style scoped>
.chip {
  display: inline-flex;
  align-items: center;
  height: 20px;
  padding: 0 8px;
  border: 1px solid var(--line-strong);
  border-radius: 999px;
  font-size: 11px;
  font-weight: 500;
  line-height: 1;
  white-space: nowrap;
  color: var(--ink-2);
}

.verified {
  border-color: var(--ink);
  background: var(--ink);
  color: var(--ground);
}

.reported {
  border-color: var(--ink-2);
  color: var(--ink);
}

.caveat {
  border-color: var(--violet);
  background: var(--violet-soft);
  color: var(--ink);
}

.untested {
  border-style: dashed;
  color: var(--ink-3);
}

.broken {
  border-color: var(--alert);
  color: var(--alert);
}

.absent {
  border-color: transparent;
  color: var(--ink-3);
}
</style>
