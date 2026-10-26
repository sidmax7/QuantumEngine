<script setup lang="ts">
import { computed } from "vue";
import { useApp } from "../stores/app";
import PanelField from "./PanelField.vue";
import ToggleSwitch from "./ToggleSwitch.vue";
import ZoneEditor from "./ZoneEditor.vue";

const app = useApp();

const lightsPending = computed(() => app.pending?.startsWith("lights:") ?? false);
const applying = computed(() => app.pending === "lighting");
/** The zones this model lights up; a Quantum 400 has only its logo. */
const zones = computed(() => {
  const m = app.state.model ?? app.state.unverified.model;
  return m?.zones.length ? m.zones : ["ring", "logo"];
});
</script>

<template>
  <div class="panel">
    <PanelField
      title="Lights"
      hint="The headset can't report its lighting, so the colours below are what was last applied from this window. Both earcups always match."
    >
      <template #aside>
        <ToggleSwitch
          label="Lights"
          :checked="app.ready && app.headset.lightsOn"
          :pending="lightsPending"
          :disabled="app.locked"
          @toggle="app.setLights"
        />
      </template>
    </PanelField>

    <div class="zones">
      <ZoneEditor v-if="zones.includes('ring')" zone="ring" title="Ring" />
      <ZoneEditor v-if="zones.includes('logo')" zone="logo" title="Logo" />
    </div>

    <div class="actions" :class="{ dirty: app.lightingDirty }">
      <button type="button" class="primary" :disabled="app.locked || !app.lightingDirty" @click="app.applyLighting">
        {{ applying ? "Applying…" : "Apply to headset" }}
      </button>
      <button v-if="app.lightingDirty && !applying" type="button" class="link-button" @click="app.revertLighting">
        Revert
      </button>
      <span class="note">
        {{
          !app.ready
            ? "Switch the headset on to apply."
            : app.lightingDirty
              ? "Applying switches the lights on."
              : "No changes to apply."
        }}
      </span>
    </div>
  </div>
</template>

<style scoped>
.zones {
  margin-top: 22px;
}

.actions {
  position: sticky;
  bottom: 0;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 16px;
  margin: 0 calc(-1 * var(--gutter));
  padding: 14px var(--gutter) 16px;
  border-top: 1px solid var(--line);
  background: var(--ground);
}

.note {
  font-size: 12px;
  color: var(--ink-3);
}

.actions.dirty .note {
  color: var(--ink-2);
}
</style>
