import type { APIRoute } from "astro";

export const prerender = false;

export const GET: APIRoute = async ({ request }) => {
  const upgrade = request.headers.get("upgrade");
  if (upgrade !== "websocket") {
    return new Response("Expected WebSocket upgrade", { status: 426 });
  }

  const pair = new WebSocketPair();
  const [client, server] = Object.values(pair);

  server.accept();
  server.addEventListener("message", (event) => {
    const data = typeof event.data === "string" ? event.data : "";
    if (data.startsWith("ping")) {
      server.send(`pong ${Date.now()}`);
    } else {
      server.send(`echo: ${data}`);
    }
  });

  return new Response(null, { status: 101, webSocket: client });
};
