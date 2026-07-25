import { writeResponse } from "./responses.js";
import { SocketResponse } from "./socket-response.js";

export async function writeUpgradeResponse(socket: ConstructorParameters<typeof SocketResponse>[0], request: Request, response: Response): Promise<void> {
  const outgoing = new SocketResponse(socket, request.method, response.statusText, response.body !== null);
  await writeResponse(outgoing, request, response);
}
