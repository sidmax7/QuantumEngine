<script setup lang="ts">
// One lighting zone's colours, effect, speed and brightness, edited in the draft.
// The figure repaints as you go; nothing reaches the headset until Apply.

import { computed, ref } from "vue";
import { Minus, Plus } from "lucide-vue-next";
import { useApp, type ZoneName } from "../stores/app";
import ColourWheel from "./ColourWheel.vue";

const props = defineProps<{ zone: ZoneName; title: string }>();
const app = useApp();

const z = computed(() => app.draft?.[props.zone]);
const effectLabel = computed(() => app.info.effects.find((e) => e.id === z.value?.effect)?.label ?? "Custom effect");
/** The effects this zone can run: the spins and rise are ring-only. */
const effects = computed(() => app.info.effects.filter((e) => e.zones.includes(props.zone)));
/** Solid (jbl_quantum::Effect::SOLID) is the only effect the headset shows
 *  at the colours' own brightness; every animation runs its own curve. */
const dims = computed(() => z.value?.effect === 1);

/** The colour whose wheel is open, if any. */
const editing = ref<number | null>(null);

function setColour(i: number, value: string) {
  if (z.value) z.value.colours[i] = value;
}

function toggle(i: number) {
  editing.value = editing.value === i ? null : i;
}

function addColour() {
  const zone = z.value;
  if (!zone || zone.colours.length >= app.info.maxColours) return;
  zone.colours.push(zone.colours[zone.colours.length - 1] ?? "#ffffff");
  editing.value = zone.colours.length - 1;
}

function removeColour() {
  const zone = z.value;
  if (!zone || zone.colours.length <= 1) return;
  zone.colours.pop();
  if (editing.value !== null && editing.value >= zone.colours.length) editing.value = null;
}
</script>

<template>
  <section v-if="z" class="zone">
    <div class="head">
      <h3>{{ title }}</h3>
      <span class="reading num">{{ effectLabel }} · speed {{ z.speed }}<template v-if="dims"> · {{ z.brightness }}%</template></span>
    </div>

    <div class="row">
      <span class="name">Colours</span>
      <div class="swatches">
        <button
          v-for="(c, i) in z.colours"
          :key="i"
          type="button"
          class="swatch"
          :class="{ on: editing === i }"
          :style="{ '--c': c }"
          :title="c"
          :aria-label="`${title} colour ${i + 1}, ${c}`"
          :aria-expanded="editing === i"
          @click="toggle(i)"
        />
        <button
          v-if="z.colours.length > 1"
          type="button"
          class="icon-button"
          :aria-label="`Remove the last ${title.toLowerCase()} colour`"
          title="Remove the last colour"
          @click="removeColour"
        >
          <Minus :size="15" :stroke-width="1.75" />
        </button>
        <button
          v-if="z.colours.length < app.info.maxColours"
          type="button"
          class="icon-button"
          :aria-label="`Add a ${title.toLowerCase()} colour`"
          title="Add a colour to cycle through"
          @click="addColour"
        >
          <Plus :size="15" :stroke-width="1.75" />
        </button>
      </div>
    </div>

    <ColourWheel
      v-if="editing !== null && z.colours[editing]"
      :model-value="z.colours[editing]"
      :label="`${title} colour ${editing + 1}`"
      @update:model-value="setColour(editing!, $event)"
    />

    <div class="row">
      <label class="name" :for="`${zone}-effect`">Effect</label>
      <div class="select-wrap">
        <select :id="`${zone}-effect`" v-model.number="z.effect">
          <option v-for="e in effects" :key="e.id" :value="e.id">{{ e.label }}</option>
          <option v-if="!effects.some((e) => e.id === z!.effect)" :value="z.effect">Custom effect</option>
        </select>
      </div>
    </div>

    <div class="row">
      <label class="name" :for="`${zone}-speed`">Speed</label>
      <input
        :id="`${zone}-speed`"
        v-model.number="z.speed"
        class="range"
        type="range"
        min="25"
        max="100"
        step="25"
        :style="{ '--fill': `${((z.speed - 25) / 75) * 100}%` }"
      />
    </div>

    <div class="row">
      <label class="name" :for="`${zone}-brightness`">Brightness</label>
      <div class="control">
        <input
          :id="`${zone}-brightness`"
          v-model.number="z.brightness"
          class="range"
          type="range"
          min="0"
          max="100"
          step="5"
          :disabled="!dims"
          :aria-describedby="dims ? undefined : `${zone}-brightness-note`"
          :style="{ '--fill': `${z.brightness}%` }"
        />
        <span v-if="!dims" :id="`${zone}-brightness-note`" class="note">
          Only Solid can be dimmed: the headset runs its effects at full brightness.
        </span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.zone {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 14px 0 16px;
  border-top: 1px solid var(--line);
}

.head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
}

h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}

.reading {
  font-size: 13px;
  color: var(--ink-2);
}

.row {
  display: grid;
  grid-template-columns: 84px 1fr;
  align-items: center;
  gap: 12px;
  min-height: 34px;
}

.name {
  font-size: 13px;
  color: var(--ink-3);
}

.swatches {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.swatch {
  width: 30px;
  height: 30px;
  padding: 0;
  border: 0;
  border-radius: 50%;
  background: var(--c);
  box-shadow:
    inset 0 0 0 1px rgb(255 255 255 / 0.18),
    0 0 0 1px var(--line);
  cursor: pointer;
  transition: transform 160ms var(--ease-out);
}

.swatch:hover {
  transform: scale(1.08);
}

.swatch.on {
  box-shadow:
    0 0 0 2px var(--ground),
    0 0 0 4px var(--ink);
}

.swatch:focus-visible {
  outline: 2px solid var(--violet);
  outline-offset: 4px;
}

.icon-button {
  display: inline-grid;
  place-items: center;
  width: 30px;
  height: 30px;
  padding: 0;
  border: 1px dashed var(--line-strong);
  border-radius: 50%;
  background: transparent;
  color: var(--ink-2);
  transition:
    border-color 160ms,
    color 160ms;
}

.icon-button:hover {
  border-color: var(--ink-2);
  color: var(--ink);
}

.select-wrap {
  position: relative;
}

.select-wrap::after {
  content: "";
  position: absolute;
  top: 50%;
  right: 13px;
  width: 6px;
  height: 6px;
  border-right: 1.5px solid var(--ink-2);
  border-bottom: 1.5px solid var(--ink-2);
  transform: translateY(-70%) rotate(45deg);
  pointer-events: none;
}

select {
  width: 100%;
  height: 34px;
  padding: 0 34px 0 11px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-sm);
  background: var(--ground);
  appearance: none;
  cursor: pointer;
}

select:hover {
  border-color: var(--ink-3);
}

.control {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.note {
  font-size: 12px;
  color: var(--ink-3);
}
</style>
