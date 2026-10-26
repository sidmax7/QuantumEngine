<script setup lang="ts">
import { computed } from "vue";
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
      title="Virtual surround 7.1"
      hint="Adds a 7.1 output, “QuantumEngine Surround 7.1”, and makes it your default; off switches back to the headset. Games set to 7.1 sound from all around you, and stereo plays from two speakers in front. The EQ's headset tuning switches to JBL surround with it, as on Windows."
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
