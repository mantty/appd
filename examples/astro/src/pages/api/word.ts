import type { APIRoute } from "astro";

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
  const word = WORDS[seed % WORDS.length];

  return new Response(JSON.stringify({ word, seed }), {
    headers: { "content-type": "application/json" },
  });
};
