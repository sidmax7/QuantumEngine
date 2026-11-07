<script setup lang="ts">
import { computed, ref } from "vue";
import { CircleAlert } from "lucide-vue-next";
import { useApp } from "../stores/app";
import PanelField from "./PanelField.vue";
import SegmentedControl from "./SegmentedControl.vue";
import ToggleSwitch from "./ToggleSwitch.vue";

const app = useApp();

const NOISE = [
  { value: "off", label: "Off" },
  { value: "on", label: "ANC" },
  { value: "talkthru", label: "TalkThru" },
];
const SIDETONE = [
  { value: "off", label: "Off" },
  { value: "low", label: "Low" },
  { value: "mid", label: "Mid" },
  { value: "high", label: "High" },
];

function pendingOf(prefix: string): string | null {
  const p = app.pending;
  return p?.startsWith(`${prefix}:`) ? p.slice(prefix.length + 1) : null;
}

/** How far the volume goes, and where the boost starts, as in `quantumengined`'s volume. */
const MAX_VOLUME = 150;
const BOOST_FROM = 100;

const volume = computed(() => app.state.volume);
/** The slider's value while it is being dragged, so a poll can't pull the thumb back. */
const dragging = ref<number | null>(null);
const shown = computed(() => dragging.value ?? volume.value.percent);
const boosted = computed(() => shown.value > BOOST_FROM);
let sending = false;
let latest: number | null = null;

/** Sends the newest value, one at a time, so a fast drag never queues a backlog. */
async function pushVolume(percent: number) {
  latest = percent;
  if (sending) return;
  sending = true;
  while (latest !== null) {
    const next = latest;
    latest = null;
    await app.setVolume(next);
  }
  sending = false;
}

function onVolumeInput(event: Event) {
  const percent = Number((event.target as HTMLInputElement).value);
  dragging.value = percent;
  void pushVolume(percent);
}

const surround = computed(() => app.state.surround);
const surroundOn = computed(() => app.serviceUp && surround.value.enabled);

const anc = computed(() => (app.ready ? app.headset.anc : ""));
const sidetone = computed(() => (app.ready ? app.headset.sidetone : ""));

const noiseHint = computed(() => {
  switch (pendingOf("anc") ?? anc.value) {
    case "on":
      return "Cancels the noise around you.";
    case "talkthru":
      return "Lets your surroundings through, so you can hear people without taking the headset off.";
    case "off":
      return "No noise processing.";
    default:
      return "Switch the headset on to change this.";
  }
});
</script>

<template>
  <div class="panel">
    <PanelField v-if="!app.wired" title="Noise control" :hint="noiseHint">
      <SegmentedControl
        label="Noise control"
        :options="NOISE"
        :value="anc"
        :pending="pendingOf('anc')"
        :disabled="app.locked"
        @select="app.setAnc"
      />
    </PanelField>
    <PanelField title="Sidetone" hint="How much of your own voice you hear while you talk.">
      <SegmentedControl
        label="Sidetone"
        :options="SIDETONE"
        :value="sidetone"
        :pending="pendingOf('sidetone')"
        :disabled="app.locked"
        @select="app.setSidetone"
      />
    </PanelField>
    <PanelField
      v-if="volume.available"
      title="Volume"
      hint="The headset's output volume, the same one your desktop's volume control moves. Past 100% it is a boost: quiet sound gets louder, but loud passages can distort."
    >
      <template #aside>
        <span class="reading num" :class="{ boost: boosted }">{{ shown }}%<template v-if="boosted"> boost</template></span>
      </template>
      <div class="volume">
        <input
          class="range"
          type="range"
          aria-label="Volume"
          :aria-valuetext="boosted ? `${shown}%, boosted` : `${shown}%`"
          min="0"
          :max="MAX_VOLUME"
          step="1"
          :value="shown"
          :disabled="!app.serviceUp"
          :style="{ '--fill': `${(shown / MAX_VOLUME) * 100}%`, '--boost': `${(BOOST_FROM / MAX_VOLUME) * 100}%` }"
          @input="onVolumeInput"
          @change="dragging = null"
        />
        <span class="mark" aria-hidden="true" :style="{ '--p': BOOST_FROM / MAX_VOLUME }">100%</span>
      </div>
    </PanelField>
    <PanelField
      title="Virtual surround 7.1"
      hint="Adds a 7.1 output, “QuantumEngine Surround 7.1”, and makes it your default; off switches back to the headset. Games set to 7.1 sound from all around you, and stereo plays from two speakers in front. On the 810, the EQ's headset tuning switches to JBL surround with it, as on Windows."
    >
      <template #aside>
        <ToggleSwitch
          label="Virtual surround 7.1"
          :checked="surroundOn"
          :pending="app.pending?.startsWith('surround:') ?? false"
          :disabled="!app.serviceUp || app.pending !== null"
          @toggle="app.setSurround"
        />
      </template>
      <p v-if="surround.status && surroundOn" class="status"><CircleAlert :size="15" :stroke-width="1.75" />{{ surround.status }}</p>
    </PanelField>
  </div>
</template>

<style scoped>
.reading {
  font-size: 13px;
  color: var(--ink-2);
}

.reading.boost {
  color: var(--violet);
}

.volume {
  position: relative;
  padding-bottom: 16px;
}

/* Under the slider, where 100% is. --p is that as a fraction of the range.
   The thumb is 16px wide, so its centre runs from 8px in to 8px short of the end. */
.mark {
  position: absolute;
  bottom: 0;
  left: calc(var(--p) * 100% + 8px - var(--p) * 16px);
  transform: translateX(-50%);
  font-size: 11px;
  color: var(--ink-3);
}

.status {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  margin: 0;
  font-size: 13px;
  color: var(--alert);
}

.status svg {
  flex: none;
  margin-top: 1px;
}
</style>
