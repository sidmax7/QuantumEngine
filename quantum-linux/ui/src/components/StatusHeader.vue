<script setup lang="ts">
// The battery as one large figure, and what the headset link is doing.

import { computed } from "vue";
import { LOW_BATTERY, useApp } from "../stores/app";

const app = useApp();

const battery = computed(() => (app.ready && !app.wired ? app.headset.battery : null));
const low = computed(() => battery.value !== null && battery.value <= LOW_BATTERY);

/** A short state, and a sentence on what to do about it. */
const status = computed((): { title: string; detail: string; live: boolean } => {
  const s = app.state;
  if (s.service === "connecting") return { title: "Connecting", detail: "Starting the background service…", live: false };
  if (s.service === "down") return { title: "Service stopped", detail: s.serviceError || "The background service isn't running.", live: false };
  switch (s.headset.link) {
    case "ready":
      if (app.wired) return { title: "Connected", detail: "Plugged in over USB", live: true };
      return { title: "Connected", detail: s.headset.deviceName ? `Paired as ${s.headset.deviceName}` : "Paired", live: true };
    case "headset_off":
      return { title: "Headset off", detail: "The dongle is plugged in. Switch the headset on, or bring it closer.", live: false };
    case "unverified": {
      const m = s.unverified.model;
      const name = m?.name || s.unverified.name || "A device";
      if (m && m.kind !== "headset") {
        return { title: "No headset", detail: `${name} is a ${m.kind}. Plug in the headset's dongle.`, live: false };
      }
      return {
        title: "Unverified model",
        detail: `${name} (${s.unverified.id}) isn't a verified model. Open Device to check it.`,
        live: false,
      };
    }
    case "no_access":
      return {
        title: "No access",
        detail: "The dongle is plugged in but can't be opened. Install the udev rule, then replug it.",
        live: false,
      };
    default:
      return { title: "No dongle", detail: "Plug in the headset's USB dongle.", live: false };
  }
});
</script>

<template>
  <header class="status">
    <div v-if="app.wired" class="battery empty">
      <span class="figure num">USB</span>
      <span class="label">Wired, no battery</span>
    </div>
    <div v-else class="battery" :class="{ low, empty: battery === null }">
      <span class="figure num" aria-hidden="true">{{ battery ?? "--" }}</span>
      <span v-if="battery !== null" class="unit" aria-hidden="true">%</span>
      <span class="visually-hidden">Battery {{ battery === null ? "unknown" : `${battery} percent` }}</span>
      <span class="label">{{ low ? "Battery low" : "Battery" }}</span>
    </div>
    <div class="link" role="status">
      <span class="title"><span class="dot" :class="{ live: status.live }" />{{ status.title }}</span>
      <span class="detail">{{ status.detail }}</span>
    </div>
  </header>
</template>

<style scoped>
.status {
  display: grid;
  grid-template-columns: auto 1fr;
  gap: 16px;
  align-items: end;
  padding: 14px 16px 12px;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  background: linear-gradient(135deg, var(--film-raised), var(--film));
}

.battery {
  display: grid;
  grid-template-columns: auto auto;
  grid-template-areas:
    "figure unit"
    "label label";
  align-items: start;
  column-gap: 2px;
}

.figure {
  grid-area: figure;
  font-size: 60px;
  line-height: 0.86;
  font-weight: 250;
  font-variation-settings: "wdth" 112;
  letter-spacing: -0.035em;
  color: var(--ink);
}

.unit {
  grid-area: unit;
  margin-top: 6px;
  font-size: 18px;
  font-weight: 400;
  font-variation-settings: "wdth" 112;
  color: var(--ink-2);
}

.label {
  grid-area: label;
  margin-top: 10px;
  font-size: 12px;
  color: var(--ink-3);
}

@media (max-width: 839px) and (min-height: 560px) {
  .figure {
    font-size: clamp(48px, 7vh, 68px);
  }
}

.empty .figure,
.empty .unit {
  color: var(--line-strong);
}

.low .figure,
.low .label {
  color: var(--alert);
}

.link {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 3px;
  min-width: 0;
  padding-bottom: 1px;
  text-align: right;
}

.title {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  font-weight: 600;
}

.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  border: 1.5px solid var(--ink-3);
}

.dot.live {
  border-color: var(--ink);
  background: var(--ink);
  box-shadow: 0 0 0 3px rgb(236 232 225 / 0.12);
}

.detail {
  max-width: 26ch;
  font-size: 12px;
  color: var(--ink-3);
  text-wrap: pretty;
}

@media (max-height: 559px) {
  .status {
    padding: 8px 10px;
    gap: 10px;
  }

  .figure {
    font-size: 48px;
  }

  .label {
    margin-top: 5px;
  }

  .link {
    padding-left: 0;
  }
}
</style>
