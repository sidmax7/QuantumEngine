<script setup lang="ts">
import { computed } from "vue";
import { FileUp, Pencil, Plus } from "lucide-vue-next";
import { useApp } from "../stores/app";
import EqDock from "./EqDock.vue";
import EqEditor from "./EqEditor.vue";
import PanelField from "./PanelField.vue";
import SegmentedControl from "./SegmentedControl.vue";

const app = useApp();

const CORRECTIONS = [
  { value: "off", label: "Off" },
  { value: "stereo", label: "JBL stereo" },
  { value: "surround", label: "JBL surround" },
];
/** The profiles that aren't for one particular game. */
const GENERAL = new Set(["Flat", "Bass Boost", "Cinematic", "FPS", "MOBA", "RPG"]);

const eq = computed(() => app.state.eq);
const usable = computed(() => app.serviceUp && eq.value.profiles.length > 0);
const busy = computed(() => app.pending !== null);

const groups = computed(() => {
  const mine = new Set(eq.value.custom);
  return [
    { name: "General", profiles: eq.value.profiles.filter((p) => GENERAL.has(p) && !mine.has(p)) },
    { name: "Games", profiles: eq.value.profiles.filter((p) => !GENERAL.has(p) && !mine.has(p)) },
  ];
});

const editing = computed(() => app.editor !== null);

function pendingOf(prefix: string): string | null {
  const p = app.pending;
  return p?.startsWith(`${prefix}:`) ? p.slice(prefix.length + 1) : null;
}

const pendingProfile = computed(() => pendingOf("eqprofile"));
</script>

<template>
  <div class="panel">
    <EqDock v-if="!app.wide" />

    <EqEditor v-if="editing" class="editor-block" />
    <PanelField v-else title="Profile" :reading="eq.profile">
      <div v-for="g in groups" :key="g.name" class="group">
        <h4>{{ g.name }}</h4>
        <div class="profiles" role="radiogroup" :aria-label="`${g.name} profiles`" @mouseleave="app.eqHover = null">
          <button
            v-for="p in g.profiles"
            :key="p"
            type="button"
            role="radio"
            class="profile"
            :aria-checked="p === eq.profile"
            :class="{ on: p === eq.profile, pending: p === pendingProfile }"
            :disabled="!usable || busy"
            @mouseenter="app.eqHover = p"
            @focus="app.eqHover = p"
            @blur="app.eqHover = null"
            @click="p !== eq.profile && app.setEqProfile(p)"
          >
            <span class="mark" aria-hidden="true" />{{ p }}
          </button>
        </div>
      </div>
      <div class="group">
        <div class="group-head">
          <h4>Yours</h4>
          <div class="group-actions">
            <button
              type="button"
              class="small-action"
              :disabled="!usable || busy"
              :title="`Start from ${eq.profile}`"
              @click="app.openEditor(eq.profile, false)"
            >
              <Plus :size="14" :stroke-width="1.75" />New
            </button>
            <button
              type="button"
              class="small-action"
              :disabled="!usable || busy"
              title="AutoEQ or Equalizer APO ParametricEQ.txt, or an EasyEffects preset"
              @click="app.importProfile"
            >
              <FileUp :size="14" :stroke-width="1.75" />{{ app.pending === "eqedit:import" ? "Importing…" : "Import…" }}
            </button>
          </div>
        </div>
        <div v-if="eq.custom.length" class="profiles mine" role="radiogroup" aria-label="Your profiles" @mouseleave="app.eqHover = null">
          <div v-for="p in eq.custom" :key="p" class="mine-row">
            <button
              type="button"
              role="radio"
              class="profile"
              :aria-checked="p === eq.profile"
              :class="{ on: p === eq.profile, pending: p === pendingProfile }"
              :disabled="!usable || busy"
              @mouseenter="app.eqHover = p"
              @focus="app.eqHover = p"
              @blur="app.eqHover = null"
              @click="p !== eq.profile && app.setEqProfile(p)"
            >
              <span class="mark" aria-hidden="true" />{{ p }}
            </button>
            <button
              type="button"
              class="edit"
              :aria-label="`Edit ${p}`"
              title="Edit"
              :disabled="!usable || busy"
              @click="app.openEditor(p, true)"
            >
              <Pencil :size="14" :stroke-width="1.75" />
            </button>
          </div>
        </div>
        <p v-else class="mine-empty">
          Make your own from the current profile, or import a curve from
          <span class="nowrap">AutoEQ</span>, Equalizer APO or EasyEffects.
        </p>
      </div>
    </PanelField>

    <PanelField
      v-if="!editing && app.tuned"
      title="Headset tuning"
      hint="JBL's own correction for the Quantum 810, under the profile. JBL stereo is a gentle bass cut; JBL surround is the curve it uses with spatial sound."
    >
      <SegmentedControl
        label="Headset tuning"
        :options="CORRECTIONS"
        :value="eq.correction"
        :pending="pendingOf('eqcorrection')"
        :disabled="!usable || busy"
        @select="app.setEqCorrection"
      />
    </PanelField>

  </div>
</template>

<style scoped>
.group + .group {
  margin-top: 6px;
}

h4 {
  margin: 0 0 6px;
  font-size: 12px;
  font-weight: 500;
  color: var(--ink-3);
}

.profiles {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 2px 8px;
}

.profile {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 34px;
  padding: 0 10px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--ink-2);
  text-align: left;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  transition:
    background 160ms,
    color 160ms;
}

.profile:hover:not(:disabled) {
  background: var(--violet-soft);
  color: var(--ink);
}

.profile.on {
  color: var(--ink);
  font-weight: 600;
}

.profile:disabled:not(.on) {
  color: var(--ink-3);
}

.mark {
  flex: none;
  width: 14px;
  height: 14px;
  border: 1.5px solid var(--line-strong);
  border-radius: 50%;
  transition:
    border-color 160ms,
    box-shadow 160ms;
}

.profile.on .mark {
  border-color: var(--ink);
  box-shadow: inset 0 0 0 3px var(--ground), inset 0 0 0 7px var(--ink);
}

.profile.pending .mark {
  border-color: var(--violet);
  animation: pulse 900ms ease-in-out infinite alternate;
}

@keyframes pulse {
  to {
    box-shadow: inset 0 0 0 3px var(--ground), inset 0 0 0 7px var(--violet);
  }
}

.group-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 6px;
}

.group-head h4 {
  margin: 0;
}

.group-actions {
  display: flex;
  gap: 4px;
}

.small-action {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 28px;
  padding: 0 9px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--ink-2);
  font-size: 12px;
  font-weight: 500;
  transition:
    border-color 160ms,
    color 160ms;
}

.small-action:hover:not(:disabled) {
  border-color: var(--ink-3);
  color: var(--ink);
}

.small-action:disabled {
  opacity: 0.4;
}

.mine-row {
  display: flex;
  align-items: center;
  min-width: 0;
}

.mine-row .profile {
  flex: 1;
  min-width: 0;
}

.edit {
  display: grid;
  place-items: center;
  flex: none;
  width: 30px;
  height: 30px;
  padding: 0;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--ink-3);
  opacity: 0;
  transition:
    opacity 120ms,
    color 120ms;
}

.mine-row:hover .edit,
.edit:focus-visible {
  opacity: 1;
}

.edit:hover {
  color: var(--ink);
  background: var(--film-raised);
}

.mine-empty {
  margin: 0;
  font-size: 12px;
  color: var(--ink-3);
  text-wrap: pretty;
}

.nowrap {
  white-space: nowrap;
}

.editor-block {
  margin-top: 26px;
}

</style>
