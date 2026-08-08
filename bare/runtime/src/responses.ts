import { responseBody, type ResponseWriter } from "./streams.js";

export interface HttpResponseWriter extends ResponseWriter {
  writeHead(status: number, headers: Readonly<Record<string, string | string[]>>): void;
}

export async function writeResponse(
  outgoing: HttpResponseWriter,
  request: Request,
  response: Response,
  headers = responseHeaders(response),
): Promise<void> {
  outgoing.writeHead(response.status, headers);
  if (request.method === "HEAD" || response.body === null) {
    outgoing.end();
    return;
  }
  await responseBody(response.body, outgoing);
}

export function responseHeaders(response: Response): Record<string, string | string[]> {
  const headers: Record<string, string | string[]> = Object.fromEntries(response.headers);
  const cookies = response.headers.getSetCookie();
  if (cookies.length === 1) headers["set-cookie"] = cookies[0] ?? "";
  if (cookies.length > 1) headers["set-cookie"] = cookies;
  return headers;
}
