import type { APIRoute } from "astro";

export const prerender = false;

export const GET: APIRoute = ({ request }) => {
  const url = new URL(request.url);

  return new Response(JSON.stringify({ origin: `${url.protocol}//${url.host}` }), {
    headers: { "content-type": "application/json" },
  });
};
