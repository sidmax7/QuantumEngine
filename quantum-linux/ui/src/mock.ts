// A stand-in backend for working on the interface in a plain browser
// (`npm run dev`, then open http://localhost:1420/?scenario=ready). Only
// loaded by the dev server outside Tauri; see main.ts. The data is made up.
//
// Scenarios: ready, custom, update, outdated, low, off, nodongle, noaccess, down,
// unverified, unverifiedAccess, unverifiedUse, unverifiedMic.
// `&editor` opens the EQ profile editor on the selected profile.
// Add `&section=lights|eq|device` to open another section.

import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type {
  FeatureDto,
  ModelDto, InfoDto, LightingDto, StateDto } from "./bindings";

const PROFILES = [
  "Flat", "Bass Boost", "Cinematic", "FPS", "MOBA", "RPG", "Apex Legends", "CS2", "Dota 2",
  "Escape from Tarkov", "Fortnite", "GTA 5", "League of Legends", "LN3 Immersion", "LN3 Thrill",
  "Overwatch 2", "PUBG", "Valorant", "WoW",
];

const FREQS = Array.from({ length: 64 }, (_, i) => 20 * 1000 ** (i / 63));

/** Rough made-up curves: a few bells per profile, on a log axis. */
const SHAPES: Record<string, [number, number, number][]> = {
  Flat: [],
  "Bass Boost": [[60, 6, 1.2], [180, 2.5, 1]],
  Cinematic: [[50, 4, 1], [3000, 2, 1], [12000, 3, 1.2]],
  FPS: [[80, -3, 1], [2500, 4, 1.3], [6000, 3, 1.5]],
  MOBA: [[120, 2, 1], [1500, 2.5, 1], [8000, 2, 1]],
  RPG: [[60, 3, 1], [800, 1.5, 1], [10000, 2.5, 1]],
};

function curve(profile: string, correction: string): number[] {
  const shape = SHAPES[profile] ?? [[100, 2, 1], [2000 + (profile.length * 317) % 4000, 3, 1.3], [9000, -2, 1]];
  const bells: [number, number, number][] = [...shape];
  if (correction === "stereo") bells.push([200, -1.6, 0.9]);
  if (correction === "surround") bells.push([13000, 7, 1], [5000, -4, 1.4]);
  return FREQS.map((f) =>
    bells.reduce((sum, [fc, g, width]) => sum + g * Math.exp(-((Math.log2(f / fc) / width) ** 2)), 0),
  );
}

const LABELS: Record<string, string> = {
  verified: "Verified",
  reported: "Reported working",
  caveat: "Works, with a catch",
  untested: "Untested",
  broken: "Doesn't work",
  absent: "Not on this model",
};
const FEATURES: [string, string][] = [
  ["reads", "Battery and status"],
  ["anc", "Noise control"],
  ["sidetone", "Sidetone"],
  ["lights", "Lights on/off"],
  ["lighting", "Lighting effects"],
  ["eq", "Equalizer"],
];
function features(statuses: Record<string, [string, string?, string?]>): FeatureDto[] {
  return FEATURES.map(([id, label]) => {
    const [status, note = "", source = ""] = statuses[id] ?? ["untested"];
    return { id, label, status, statusLabel: LABELS[status]!, note, source };
  });
}
const Q810: ModelDto = {
  id: "0ecb:2069",
  name: "JBL Quantum 810 Wireless",
  family: "Wireless flagship",
  kind: "headset",
  verified: true,
  zones: ["logo", "ring"],
  features: features(Object.fromEntries(FEATURES.map(([id]) => [id, ["verified", "", "tested by the project"]]))),
};
const Q800: ModelDto = {
  id: "0ecb:203e",
  name: "JBL Quantum 800",
  family: "Wireless flagship",
  kind: "headset",
  verified: false,
  zones: ["logo", "ring"],
  features: features({
    reads: ["reported", "Every read answers; it reports no serial number.", "issue #2"],
    anc: ["caveat", "Only with the mic boom raised; with it down the headset plays an error tone.", "issue #2"],
    sidetone: ["reported", "", "issue #2"],
    lights: ["reported", "", "issue #2"],
    lighting: ["untested", "Rebuilt in 0.4.5 to work the way JBL's own app drives the 800; not tried yet."],
  }),
};
const Q400: ModelDto = {
  id: "0ecb:20ee",
  name: "JBL Quantum 400",
  family: "Wired",
  kind: "headset",
  verified: false,
  zones: ["logo"],
  features: features({
    reads: ["reported", "Mic, dial, lights, serial and firmware answer; being wired, it has no battery or wireless link.", "issue #4"],
    anc: ["absent", "", "JBL's app has none for it"],
    lighting: ["untested", "JBL's own app drives its logo the same way as the 810's lights."],
    eq: ["caveat", "Choose QuantumEngine as the sound output yourself; it only attaches to the 810 on its own.", "issue #4"],
  }),
};
const STREAM: ModelDto = {
  id: "0ecb:205f",
  name: "JBL Quantum Stream",
  family: "Microphone",
  kind: "microphone",
  verified: false,
  zones: [],
  features: [],
};

function scenarioState(scenario: string): StateDto {
  const unverified = scenario.startsWith("unverified");
  // `unverifiedWired`: a Quantum 400 in use, which has no battery or noise control.
  const wired = scenario === "unverifiedWired";
  const using = scenario === "unverifiedUse" || wired;
  const ready = !["off", "nodongle", "noaccess", "down"].includes(scenario) && (!unverified || using);
  return {
    service: scenario === "down" ? "down" : "up",
    serviceError: scenario === "down" ? "The background service (quantumengined) didn't start." : "",
    serviceOutdated: scenario === "outdated",
    headset: {
      link: ready
        ? "ready"
        : unverified
          ? "unverified"
          : scenario === "off"
            ? "headset_off"
            : scenario === "noaccess"
              ? "no_access"
              : "no_dongle",
      linkDetail: "",
      battery: scenario === "low" ? 11 : 82,
      anc: "on",
      sidetone: "low",
      micActive: true,
      dial: 11,
      lightsOn: true,
      deviceName: "PC-cachyos #1",
      serial: ready ? "SN-0000-DEMO" : "",
      firmware: ready ? ["0.7.2", "0.8.2", "1.0.2", "0.7.2"] : [],
      busy: false,
      lastError: "",
    },
    eq:
      scenario === "outdated"
        ? { enabled: false, profile: "", profiles: [], correction: "", status: "", custom: [], previewing: false }
        : {
            enabled: true,
            profile: scenario === "custom" ? "Night Curve" : "CS2",
            profiles: [...PROFILES, "Night Curve"],
            correction: "stereo",
            status: "",
            custom: ["Night Curve"],
            previewing: false,
          },
    surround: { enabled: false, status: "" },
    unverified:
      scenario === "unverifiedMic"
        ? { id: "0ecb:205f", name: "JBL Quantum Stream", access: "", inUse: false, allowed: [], model: STREAM }
        : unverified
          ? {
              id: wired ? "0ecb:20ee" : "0ecb:203e",
              name: wired ? "JBL Quantum 400" : "JBL Quantum 800",
              access: scenario === "unverifiedAccess" ? "cannot open /dev/hidraw3: Permission denied (os error 13)" : "",
              inUse: using,
              allowed: wired ? ["sidetone", "lights"] : using ? ["anc"] : [],
              model: wired ? Q400 : Q800,
            }
          : { id: "", name: "", access: "", inUse: false, allowed: [], model: null },
    model: ready ? (wired ? Q400 : using ? Q800 : Q810) : null,
    update: {
      serviceVersion: scenario === "outdated" ? "" : "0.3.0",
      available: scenario === "update" ? "0.3.1" : "",
      canInstall: true,
      status: "",
      busy: false,
      checks: true,
    },
  };
}

export function installMock(scenario: string) {
  let state = scenarioState(scenario);
  let lighting: LightingDto = {
    ring: { brightness: 90, speed: 50, colours: ["#8a3cff", "#2fb7ff"], effect: 4 },
    logo: { brightness: 100, speed: 50, colours: ["#ffffff"], effect: 0 },
  };
  const info: InfoDto = {
    version: "0.3.0",
    effects: (
      [
        [1, "Solid", true], [0, "Breathing", true], [2, "Wave", true], [3, "Glitch", true],
        [4, "Spin clockwise", false], [5, "Spin counter clockwise", false], [6, "Rise", false],
      ] as [number, string, boolean][]
    ).map(([id, label, logo]) => ({ id, label, zones: logo ? ["logo", "ring"] : ["ring"] })),
    maxColours: 10,
    eqFrequencies: FREQS,
    eqMaxBands: 18,
  };
  type MockBand = { kind: string; freq: number; gain: number; q: number };
  const custom: Record<string, MockBand[]> = {
    "Night Curve": [
      { kind: "lowshelf", freq: 105, gain: -4, q: 0.7 },
      { kind: "bell", freq: 2900, gain: 3, q: 2.7 },
      { kind: "highshelf", freq: 9000, gain: -3, q: 0.7 },
      { kind: "highpass", freq: 25, gain: 0, q: 0.7 },
    ],
  };
  const bandsCurve = (bands: MockBand[]) =>
    FREQS.map((f) =>
      bands.reduce((sum, b) => {
        const oct = Math.log2(f / b.freq);
        if (b.kind === "bell") return sum + b.gain * Math.exp(-((oct * b.q) ** 2));
        if (b.kind === "lowshelf") return sum + b.gain / (1 + Math.exp(oct * 4));
        if (b.kind === "highshelf") return sum + b.gain / (1 + Math.exp(-oct * 4));
        if (b.kind === "highpass") return sum + Math.min(0, 12 * oct);
        return sum + Math.min(0, -12 * oct);
      }, 0),
    );

  const change = (f: (s: StateDto) => void, delay = 650) =>
    new Promise<void>((resolve) => {
      window.setTimeout(() => {
        const next = structuredClone(state);
        f(next);
        state = next;
        void emit("state-event", state);
        resolve();
      }, delay);
    });

  mockIPC(
    async (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "state":
          return state;
        case "info":
          return info;
        case "lighting":
          return lighting;
        case "set_anc":
          await change((s) => (s.headset.anc = a.mode as string));
          return a.mode;
        case "set_sidetone":
          await change((s) => (s.headset.sidetone = a.level as string));
          return a.level;
        case "set_lights":
          await change((s) => (s.headset.lightsOn = a.on as boolean));
          return a.on;
        case "apply_lighting":
          await change((s) => (s.headset.lightsOn = true), 900);
          lighting = a.lighting as LightingDto;
          return null;
        case "set_eq_enabled":
          await change((s) => (s.eq.enabled = a.enabled as boolean), 200);
          return a.enabled;
        case "set_eq_profile":
          await change((s) => (s.eq.profile = a.name as string), 200);
          return a.name;
        case "set_eq_correction":
          await change((s) => (s.eq.correction = a.correction as string), 200);
          return a.correction;
        case "set_surround_enabled":
          await change((s) => (s.surround.enabled = a.enabled as boolean), 500);
          return a.enabled;
        case "eq_profile_bands": {
          const name = a.name as string;
          if (custom[name]) return custom[name];
          const gains = [-6, -4, -3, -2, 2, 3, 4, 4, 3, 0];
          return [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000].map((freq, i) => ({ kind: "bell", freq, gain: gains[i], q: 1 }));
        }
        case "eq_bands_response":
          return bandsCurve(a.bands as MockBand[]);
        case "save_eq_profile": {
          const name = a.name as string;
          if (a.original && a.original !== name) delete custom[a.original as string];
          custom[name] = a.bands as MockBand[];
          await change((s) => {
            s.eq.custom = Object.keys(custom);
            s.eq.profiles = [...PROFILES, ...s.eq.custom];
            s.eq.profile = name;
            s.eq.previewing = false;
          }, 300);
          return name;
        }
        case "delete_eq_profile":
          delete custom[a.name as string];
          await change((s) => {
            s.eq.custom = Object.keys(custom);
            s.eq.profiles = [...PROFILES, ...s.eq.custom];
            if (s.eq.profile === a.name) s.eq.profile = "Flat";
          }, 300);
          return null;
        case "preview_eq_bands":
          if (!state.eq.previewing) await change((s) => (s.eq.previewing = true), 50);
          return null;
        case "end_eq_preview":
          await change((s) => (s.eq.previewing = false), 50);
          return null;
        case "import_eq_profile":
          return null;
        case "eq_response":
          if (custom[a.profile as string]) return bandsCurve(custom[a.profile as string]!);
          return curve(a.profile as string, a.correction as string);
        case "check_for_updates":
          await change((s) => (s.update.status = "QuantumEngine 0.3.0 is up to date"), 900);
          return "";
        case "install_update":
          await change((s) => (s.update.busy = true), 100);
          return null;
        case "set_update_checks":
          await change((s) => (s.update.checks = a.enabled as boolean), 200);
          return a.enabled;
        case "probe_unverified":
          await change(() => {}, 700);
          return [
            "QuantumEngine compatibility report (jbl-quantum 0.4.2)",
            'Device:   0ecb:203e "JBL Quantum 800" at /dev/hidraw3',
            "Verified: no (this library is only verified on 0ecb:2069)",
            "Reads only; nothing was written to the device.",
            "  0x68 link         answered   01                 plausible",
            "  0x49 battery      answered   4b (75%)          plausible",
            "  0x45 noise ctl    answered   01                 plausible",
            "  0x5C sidetone     answered   00                 plausible",
            "  0x67 mic          no answer  report 0x67 did not answer (it is probably write-only)",
            "  0x62 dial         answered   08 (8/16)          plausible",
            "  0x4A lights       answered   01                 plausible",
            "  0x61 serial       answered   4d 4d 30 31 36 39 2d 49 4e 30 30 35 39 36 33 37 plausible",
            "  0x51 firmware     answered   00 07 02 00 08 02 00 00 00 00 07 02 plausible",
            "  0x5B host name    answered   00 00               plausible",
            "Summary: 9 of 10 reads answered, 9 plausible.",
          ].join("\n");
        case "set_unverified":
          await change((s) => {
            s.unverified.inUse = a.inUse as boolean;
            s.unverified.allowed = a.allowed as string[];
            if (a.inUse) Object.assign(s.headset, scenarioState("ready").headset);
            else s.headset.link = "unverified";
          }, 300);
          return null;
        case "grant_unverified_access":
          await change((s) => (s.unverified.access = "Waiting for your password…"), 100);
          await change((s) => (s.unverified.access = ""), 1800);
          return null;
        case "restart_service":
          await change((s) => {
            Object.assign(s, scenarioState("ready"));
          }, 1200);
          return null;
        case "open_release_page":
        case "open_project_page":
          return null;
        default:
          throw new Error(`mock: no handler for ${cmd}`);
      }
    },
    { shouldMockEvents: true },
  );
}
