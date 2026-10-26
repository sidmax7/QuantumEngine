// The window's state: what the service reports (pushed as events), the
// facts that never change, the lighting being edited, and which command is
// in flight. Components render from here and call the actions; none of them
// talk to the backend directly.

import { defineStore } from "pinia";
import { computed, ref, shallowRef } from "vue";
import { api, type Band, type Info } from "../api";
import type { LightingDto, StateDto, ZoneDto } from "../bindings";

export type Section = "sound" | "lights" | "eq" | "device";
export type ZoneName = "logo" | "ring";

/** Battery level at or below which it's shown as low. */
export const LOW_BATTERY = 15;

const EMPTY_STATE: StateDto = {
  service: "connecting",
  serviceError: "",
  serviceOutdated: false,
  headset: {
    link: "no_dongle",
    linkDetail: "",
    battery: 0,
    anc: "",
    sidetone: "",
    micActive: false,
    dial: 8,
    lightsOn: false,
    deviceName: "",
    serial: "",
    firmware: [],
    busy: false,
    lastError: "",
  },
  eq: { enabled: false, profile: "", profiles: [], correction: "", status: "", custom: [], previewing: false },
  surround: { enabled: false, status: "" },
  update: { serviceVersion: "", available: "", canInstall: false, status: "", busy: false, checks: true },
  unverified: { id: "", name: "", access: "", inUse: false, allowed: [], model: null },
  model: null,
};

const SECTIONS: Section[] = ["sound", "lights", "eq", "device"];

/** `?section=lights` opens another section first (used by the dev mock). */
function initialSection(): Section {
  const asked = new URLSearchParams(location.search).get("section");
  return SECTIONS.find((s) => s === asked) ?? "sound";
}

/** A custom EQ profile being made or changed. */
export interface EqEditor {
  /** The custom profile being edited, or null for a new one. */
  original: string | null;
  name: string;
  bands: Band[];
}

export interface Toast {
  id: number;
  message: string;
}

function cloneLighting(l: LightingDto): LightingDto {
  return {
    logo: { ...l.logo, colours: [...l.logo.colours] },
    ring: { ...l.ring, colours: [...l.ring.colours] },
  };
}

function sameZone(a: ZoneDto, b: ZoneDto): boolean {
  return (
    a.brightness === b.brightness &&
    a.speed === b.speed &&
    a.effect === b.effect &&
    a.colours.length === b.colours.length &&
    a.colours.every((c, i) => c.toLowerCase() === b.colours[i]?.toLowerCase())
  );
}

export const useApp = defineStore("app", () => {
  const state = shallowRef<StateDto>(EMPTY_STATE);
  const info = shallowRef<Info>({ version: "", effects: [], maxColours: 10, eqFrequencies: [], eqMaxBands: 18 });
  const section = ref<Section>(initialSection());
  /** What's being changed right now, e.g. `anc:on`; one at a time. */
  const pending = ref<string | null>(null);
  const toasts = ref<Toast[]>([]);
  /** The lighting last applied from this window, and the one being edited. */
  const applied = shallowRef<LightingDto | null>(null);
  const draft = ref<LightingDto | null>(null);
  const curves = ref<Record<string, number[]>>({});
  const editor = ref<EqEditor | null>(null);
  /** The profile under the pointer in the EQ list, drawn over the curve. */
  const eqHover = ref<string | null>(null);
  /** The band selected in the editor, on the curve and in the table. */
  const eqBand = ref<number | null>(null);
  /** Wide windows keep the EQ curve beside the headset; narrow ones put it in the EQ tab. */
  const wideQuery = window.matchMedia("(min-width: 840px)");
  const wide = ref(wideQuery.matches);
  wideQuery.addEventListener("change", () => (wide.value = wideQuery.matches));

  const headset = computed(() => state.value.headset);
  const serviceUp = computed(() => state.value.service === "up");
  const ready = computed(() => serviceUp.value && headset.value.link === "ready");
  /** A USB-wired model, such as the Quantum 400: no battery or noise control. */
  const wired = computed(() => (state.value.model ?? state.value.unverified.model)?.family === "Wired");
  /** Headset controls take one change at a time. */
  const locked = computed(() => !ready.value || pending.value !== null || headset.value.busy);
  const lightingDirty = computed(() => {
    const a = applied.value;
    const d = draft.value;
    return !!a && !!d && !(sameZone(a.logo, d.logo) && sameZone(a.ring, d.ring));
  });

  let toastId = 0;
  function toast(message: string) {
    const id = ++toastId;
    toasts.value = [...toasts.value.slice(-2), { id, message }];
    window.setTimeout(() => dismiss(id), 6000);
  }
  function dismiss(id: number) {
    toasts.value = toasts.value.filter((t) => t.id !== id);
  }

  /** Runs one change, marking it pending and reporting a failure. */
  async function run(key: string, call: () => Promise<unknown>) {
    if (pending.value) return;
    pending.value = key;
    try {
      await call();
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e));
    } finally {
      pending.value = null;
    }
  }

  async function start() {
    info.value = await api.info();
    await api.onState((event) => {
      state.value = event.payload;
    });
    state.value = await api.state();
    const lighting = await api.lighting();
    applied.value = lighting;
    draft.value = cloneLighting(lighting);
    // Dev server only: `&probe` runs the unverified-model check, for screenshots.
    if (import.meta.env.DEV && new URLSearchParams(location.search).has("probe")) {
      void probeUnverified();
    }
    // Dev server only: `&editor` opens the EQ editor, for screenshots.
    if (import.meta.env.DEV && new URLSearchParams(location.search).has("editor")) {
      void openEditor(state.value.eq.profile, true);
    }
    // Dev server only: `&draft` starts with an unapplied edit, for screenshots.
    if (import.meta.env.DEV && new URLSearchParams(location.search).has("draft")) {
      draft.value.ring.colours = ["#ff4d6d", "#ffb347"];
    }
  }

  /** Drops cached curves, e.g. after a custom profile changed. */
  function forgetCurves() {
    curves.value = {};
  }

  /** Opens the editor on a copy of `from` (a new profile), or on `from`
   *  itself when it's one of the user's own and `edit` is set. */
  async function openEditor(from: string, edit: boolean) {
    try {
      const bands = await api.eqProfileBands(from);
      const own = state.value.eq.custom.includes(from);
      editor.value =
        edit && own
          ? { original: from, name: from, bands }
          : { original: null, name: from === "Flat" ? "My profile" : `${from} (mine)`, bands };
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e));
    }
  }

  async function closeEditor() {
    editor.value = null;
    if (state.value.eq.previewing) {
      await api.endEqPreview().catch(() => {});
    }
  }

  function saveEditor() {
    const e = editor.value;
    if (!e) return;
    return run("eqedit:save", async () => {
      await api.saveEqProfile(e.original ?? "", e.name, e.bands);
      editor.value = null;
      forgetCurves();
    });
  }

  function deleteProfile(name: string) {
    return run("eqedit:delete", async () => {
      await api.deleteEqProfile(name);
      if (editor.value?.original === name) editor.value = null;
      forgetCurves();
    });
  }

  function importProfile() {
    return run("eqedit:import", async () => {
      const name = await api.importEqProfile();
      if (name) forgetCurves();
    });
  }

  async function curve(profile: string, correction: string): Promise<number[]> {
    const key = `${profile}|${correction}`;
    const known = curves.value[key];
    if (known) return known;
    const points = await api.eqResponse(profile, correction);
    curves.value = { ...curves.value, [key]: points };
    return points;
  }

  const setAnc = (mode: string) => run(`anc:${mode}`, () => api.setAnc(mode));
  const setSidetone = (level: string) => run(`sidetone:${level}`, () => api.setSidetone(level));
  const setLights = (on: boolean) => run(`lights:${on}`, () => api.setLights(on));
  const setEqEnabled = (on: boolean) => run(`eq:${on}`, () => api.setEqEnabled(on));
  const setEqProfile = (name: string) => run(`eqprofile:${name}`, () => api.setEqProfile(name));
  const setEqCorrection = (c: string) => run(`eqcorrection:${c}`, () => api.setEqCorrection(c));
  /** The latest compatibility report from an unverified model. */
  const probeText = ref("");
  const probeUnverified = () =>
    run("unverified:probe", async () => {
      probeText.value = await api.probeUnverified();
    });
  const setUnverified = (inUse: boolean, allowed: string[]) => run("unverified:set", () => api.setUnverified(inUse, allowed));
  const grantAccess = () => run("unverified:grant", () => api.grantUnverifiedAccess());
  const setSurround = (on: boolean) => run(`surround:${on}`, () => api.setSurround(on));
  const checkForUpdates = () => run("update:check", () => api.checkForUpdates());
  const installUpdate = () => run("update:install", () => api.installUpdate());
  const restartService = () => run("service:restart", () => api.restartService());
  const setUpdateChecks = (on: boolean) => run(`update:checks:${on}`, () => api.setUpdateChecks(on));

  function applyLighting() {
    const d = draft.value;
    if (!d) return;
    const sent = cloneLighting(d);
    return run("lighting", async () => {
      await api.applyLighting(sent);
      applied.value = sent;
    });
  }

  function revertLighting() {
    if (applied.value) draft.value = cloneLighting(applied.value);
  }

  return {
    state,
    info,
    section,
    pending,
    toasts,
    applied,
    draft,
    headset,
    serviceUp,
    ready,
    wired,
    locked,
    lightingDirty,
    start,
    toast,
    dismiss,
    curve,
    editor,
    eqHover,
    eqBand,
    wide,
    openEditor,
    closeEditor,
    saveEditor,
    deleteProfile,
    importProfile,
    setAnc,
    setSidetone,
    setLights,
    setEqEnabled,
    setEqProfile,
    setEqCorrection,
    setSurround,
    probeText,
    probeUnverified,
    setUnverified,
    grantAccess,
    checkForUpdates,
    installUpdate,
    setUpdateChecks,
    restartService,
    applyLighting,
    revertLighting,
  };
});
