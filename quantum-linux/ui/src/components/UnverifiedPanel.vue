<script setup lang="ts">
// A device from the same vendor that QuantumEngine isn't verified on. What the
// catalogue knows about the model is shown first. Nothing is sent to a
// headset until the user says so: first it can be checked with reads only,
// then used read-only, then given writes one feature at a time. Microphones
// and earbuds are named and left alone.

import { computed, ref } from "vue";
import { Copy } from "lucide-vue-next";
import type { FeatureDto } from "../bindings";
import { useApp } from "../stores/app";
import PanelField from "./PanelField.vue";
import SupportChip from "./SupportChip.vue";
import ToggleSwitch from "./ToggleSwitch.vue";

const app = useApp();

const u = computed(() => app.state.unverified);
const model = computed(() => u.value.model);
const headset = computed(() => !model.value || model.value.kind === "headset");
const busy = computed(() => app.pending !== null);
const probing = computed(() => app.pending === "unverified:probe");
const asking = computed(() => u.value.access.startsWith("Waiting") || app.pending === "unverified:grant");
const copied = ref(false);

/** The writes that can be allowed, and the catalogue feature each one covers. */
const SWITCHES = [
  { id: "anc", feature: "anc", label: "Noise control", hint: "Off, ANC and TalkThru." },
  { id: "sidetone", feature: "sidetone", label: "Sidetone", hint: "How much of your own voice you hear." },
  { id: "lights", feature: "lighting", label: "Lights", hint: "On and off, colours and effects." },
];

function known(feature: string): FeatureDto | undefined {
  return model.value?.features.find((f) => f.id === feature);
}

/** What's on record for the lights: on/off and effects can differ. */
function lightsNote(): FeatureDto | undefined {
  const effects = known("lighting");
  const onOff = known("lights");
  if (!effects || !onOff) return effects ?? onOff;
  return effects.status === "broken" || effects.status === "caveat" ? effects : onOff;
}

function record(id: string): FeatureDto | undefined {
  return id === "lights" ? lightsNote() : known(SWITCHES.find((s) => s.id === id)!.feature);
}

/** The switches for what this model has: a wired one has no noise control. */
const switches = computed(() => SWITCHES.filter((s) => record(s.id)?.status !== "absent"));

function allow(feature: string, on: boolean) {
  const next = new Set(u.value.allowed);
  if (on) next.add(feature);
  else next.delete(feature);
  void app.setUnverified(u.value.inUse, [...next]);
}

async function copyReport() {
  try {
    await navigator.clipboard.writeText(app.probeText);
    copied.value = true;
    window.setTimeout(() => (copied.value = false), 1800);
  } catch {
    app.toast("Couldn't copy. Select the report and copy it by hand.");
  }
}
</script>

<template>
  <PanelField :title="headset ? 'Unverified headset' : 'Not a headset'">
    <div class="card">
      <p class="who">{{ model?.name || u.name || "Unknown device" }}</p>
      <p class="meta">
        <span v-if="model">{{ model.family }}</span>
        <span class="num">{{ u.id }}</span>
        <span v-if="model?.zones.length">Lights: {{ model.zones.join(", ") }}</span>
      </p>
    </div>

    <template v-if="!headset">
      <p class="lead">
        This is a {{ model?.kind }}. QuantumEngine only controls headsets, so there is nothing to set up for it, and
        nothing is sent to it.
      </p>
    </template>

    <template v-else>
      <p class="lead">
        QuantumEngine is verified on the JBL Quantum 810 Wireless only. This model might work, partly work or not work
        at all, and nothing is sent to it until you choose.
        <template v-if="!model">It isn't in QuantumEngine's list of models either.</template>
      </p>

      <div v-if="model" class="step">
        <h4>What's known about this model</h4>
        <ul class="known">
          <li v-for="f in model.features" :key="f.id">
            <span class="fname">{{ f.label }}</span>
            <SupportChip :feature="f" />
            <!-- Always there, empty or not, so each feature keeps its own row. -->
            <span class="note">
              {{ f.note }}<template v-if="f.source"> ({{ f.source }})</template>
            </span>
          </li>
        </ul>
      </div>

      <div class="step">
        <h4>1. Check what it answers</h4>
        <p class="note">
          This only reads: battery, noise control, sidetone and so on, the same questions the 810 answers. It writes
          nothing to the headset.
        </p>
        <div class="row">
          <button type="button" class="secondary" :disabled="busy" @click="app.probeUnverified">
            {{ probing ? "Checking…" : app.probeText ? "Check again" : "Check it" }}
          </button>
          <button
            type="button"
            class="secondary"
            :disabled="busy || asking"
            title="Needs your password. Adds a rule for this one device."
            @click="app.grantAccess"
          >
            {{ asking ? "Waiting for your password…" : "Allow access…" }}
          </button>
        </div>
        <p v-if="u.access && !asking" class="problem" role="status">{{ u.access }}</p>
        <template v-if="app.probeText">
          <pre class="report" tabindex="0" aria-label="Compatibility report">{{ app.probeText }}</pre>
          <div class="row">
            <button type="button" class="secondary" @click="copyReport">
              <Copy :size="14" :stroke-width="1.75" />{{ copied ? "Copied" : "Copy report" }}
            </button>
            <span class="note">Paste it in a GitHub issue to help us support this model.</span>
          </div>
        </template>
        <p v-else class="note">If it says it couldn't open the device, use Allow access first.</p>
      </div>

      <div class="step">
        <div class="switch-row">
          <h4>2. Use this headset</h4>
          <ToggleSwitch
            label="Use this unverified headset"
            :checked="u.inUse"
            :pending="app.pending === 'unverified:set'"
            :disabled="busy"
            @toggle="app.setUnverified($event, u.allowed)"
          />
        </div>
        <p class="note">
          On, QuantumEngine shows what the headset reports, read-only. Anything the headset doesn't answer is left
          blank.
        </p>
      </div>

      <div class="step" :class="{ off: !u.inUse }">
        <h4>3. Allow changes</h4>
        <p class="note">
          Each one sends commands built for the 810. Turn on only what you are happy to try; they may not behave on
          this model.
        </p>
        <div v-for="s in switches" :key="s.id" class="switch-row feature">
          <span class="what">
            <span class="title-line">
              {{ s.label }}
              <SupportChip v-if="record(s.id)" :feature="record(s.id)!" />
            </span>
            <span class="note">{{ record(s.id)?.note || s.hint }}</span>
          </span>
          <ToggleSwitch
            :label="`Allow changing ${s.label.toLowerCase()}`"
            :checked="u.allowed.includes(s.id)"
            :disabled="busy || !u.inUse"
            @toggle="allow(s.id, $event)"
          />
        </div>
      </div>
    </template>
  </PanelField>
</template>

<style scoped>
.card {
  margin: 0 0 12px;
  padding: 12px 14px;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  background: var(--film);
}

.who {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
}

.meta {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 14px;
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--ink-3);
}

.lead {
  margin: 0 0 12px;
  font-size: 13px;
  color: var(--ink-2);
  text-wrap: pretty;
}

.step {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 14px 0;
  border-top: 1px solid var(--line);
}

.step.off {
  opacity: 0.55;
}

h4 {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
}

.known {
  display: grid;
  grid-template-columns: auto auto 1fr;
  gap: 8px 12px;
  align-items: baseline;
  margin: 0;
  padding: 0;
  list-style: none;
}

.known li {
  display: contents;
}

.fname {
  font-size: 13px;
}

.known li > .note {
  grid-column: 3;
}

.known :deep(.chip) {
  justify-self: start;
}

.note {
  margin: 0;
  font-size: 12px;
  color: var(--ink-3);
  text-wrap: pretty;
}

.row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 12px;
}

.switch-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.feature {
  padding: 6px 0;
  font-size: 13px;
}

.what {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.title-line {
  display: flex;
  align-items: center;
  gap: 8px;
}

.problem {
  margin: 0;
  font-size: 13px;
  color: var(--alert);
}

.report {
  margin: 0;
  max-height: 240px;
  padding: 12px;
  overflow: auto;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  background: var(--ground);
  font: 12px/1.5 ui-monospace, "JetBrains Mono", monospace;
  color: var(--ink-2);
  white-space: pre;
}

@media (max-width: 520px) {
  .known {
    grid-template-columns: 1fr auto;
  }

  .known li > .note {
    grid-column: 1 / -1;
    margin-bottom: 4px;
  }
}
</style>
