import { resolve } from "$app/paths";

export const tagPath = (name: string) =>
  resolve("/(map)/t/[tag]", { tag: encodeURIComponent(name) });
