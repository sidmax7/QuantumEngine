// Thin layer over the generated bindings: a failed command throws an Error
// carrying the service's message, instead of returning a `{ status }`.

import { commands, events, type InfoDto, type LightingDto } from "./bindings";

/** `InfoDto` with its numbers non-null (Rust sends NaN as null; there is none). */
export type Info = Omit<InfoDto, "eqFrequencies"> & { eqFrequencies: number[] };

/** `BandDto` with its numbers non-null. */
export type Band = { kind: string; freq: number; gain: number; q: number };

type Outcome<T> = { status: "ok"; data: T } | { status: "error"; error: string };

export async function unwrap<T>(call: Promise<Outcome<T>>): Promise<T> {
  const result = await call;
  if (result.status === "ok") return result.data;
  throw new Error(result.error);
}

export const api = {
  state: () => commands.state(),
  info: (): Promise<Info> =>
    commands.info().then((i) => ({ ...i, eqFrequencies: i.eqFrequencies.map((f) => f ?? 0) })),
  onState: events.stateEvent.listen,
  setAnc: (mode: string) => unwrap(commands.setAnc(mode)),
  setSidetone: (level: string) => unwrap(commands.setSidetone(level)),
  setLights: (on: boolean) => unwrap(commands.setLights(on)),
  lighting: () => commands.lighting(),
  applyLighting: (lighting: LightingDto) => unwrap(commands.applyLighting(lighting)),
  setEqEnabled: (on: boolean) => unwrap(commands.setEqEnabled(on)),
  setEqProfile: (name: string) => unwrap(commands.setEqProfile(name)),
  setEqCorrection: (c: string) => unwrap(commands.setEqCorrection(c)),
  eqResponse: (profile: string, correction: string) =>
    unwrap(commands.eqResponse(profile, correction)).then((points) => points.map((p) => p ?? 0)),
  eqProfileBands: (name: string): Promise<Band[]> =>
    unwrap(commands.eqProfileBands(name)).then((bands) =>
      bands.map((b) => ({ kind: b.kind, freq: b.freq ?? 1000, gain: b.gain ?? 0, q: b.q ?? 1 })),
    ),
  eqBandsResponse: (bands: Band[], correction: string) =>
    unwrap(commands.eqBandsResponse(bands, correction)).then((points) => points.map((p) => p ?? 0)),
  saveEqProfile: (original: string, name: string, bands: Band[]) =>
    unwrap(commands.saveEqProfile(original, name, bands)),
  deleteEqProfile: (name: string) => unwrap(commands.deleteEqProfile(name)),
  previewEqBands: (bands: Band[]) => unwrap(commands.previewEqBands(bands)),
  endEqPreview: () => unwrap(commands.endEqPreview()),
  importEqProfile: () => unwrap(commands.importEqProfile()),
  setSurround: (on: boolean) => unwrap(commands.setSurroundEnabled(on)),
  setVolume: (percent: number) => unwrap(commands.setVolume(percent)),
  probeUnverified: () => unwrap(commands.probeUnverified()),
  setUnverified: (inUse: boolean, allowed: string[]) => unwrap(commands.setUnverified(inUse, allowed)),
  grantUnverifiedAccess: () => unwrap(commands.grantUnverifiedAccess()),
  checkForUpdates: () => unwrap(commands.checkForUpdates()),
  installUpdate: () => unwrap(commands.installUpdate()),
  setUpdateChecks: (on: boolean) => unwrap(commands.setUpdateChecks(on)),
  restartService: () => unwrap(commands.restartService()),
  openReleasePage: () => unwrap(commands.openReleasePage()),
  openProjectPage: () => unwrap(commands.openProjectPage()),
};
