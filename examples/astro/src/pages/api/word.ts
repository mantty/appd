import type { APIRoute } from "astro";
import wordPickerModule from "../../wasm/word_picker.wasm";

export const prerender = false;

const WORDS = [
  "aurora",
  "comet",
  "ember",
  "glacier",
  "harbor",
  "lumen",
  "meadow",
  "nimbus",
  "opal",
  "quartz",
];

export const GET: APIRoute = async () => {
  const seed = crypto.getRandomValues(new Uint32Array(1))[0];

  const instance = await WebAssembly.instantiate(wordPickerModule);
  const { pick_index } = instance.exports as {
    pick_index: (seed: number, count: number) => number;
  };
  const word = WORDS[pick_index(seed, WORDS.length)];

  return new Response(JSON.stringify({ word, seed }), {
    headers: { "content-type": "application/json" },
  });
};
