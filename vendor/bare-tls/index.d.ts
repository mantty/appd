import { Duplex, DuplexEvents } from 'bare-stream'
import EventEmitter from 'bare-events'
import constants from './lib/constants'
import TLSError from './lib/errors'

export { constants, TLSError as errors }

export interface TLSSocketEvents extends DuplexEvents {
  connect: []
}

export interface TLSSocketOptions {
  isServer?: boolean
  cert?: ArrayBufferView
  key?: ArrayBufferView
  host?: string
  rejectUnauthorized?: boolean
  ca?: ArrayBufferView
  alpnProtocols?: string[]
  eagerOpen?: boolean
  allowHalfOpen?: boolean
  readBufferSize?: number
}

export interface TLSSocket<M extends TLSSocketEvents = TLSSocketEvents> extends Duplex<M> {
  readonly socket: Duplex
  readonly encrypted: true
  readonly alpnProtocol: string | null
}

export class TLSSocket {
  constructor(socket: Duplex, opts?: TLSSocketOptions)
}

export { TLSSocket as Socket }

export interface TLSNetServerEvents {
  listening: []
  connection: [socket: TLSSocket]
  error: [err: Error]
  close: []
}

export interface TLSNetServer extends EventEmitter<TLSNetServerEvents> {
  readonly listening: boolean
}

export function createServer(
  opts?: TLSSocketOptions,
  onconnection?: (socket: TLSSocket) => void
): TLSNetServer

export function createConnection(
  opts: TLSSocketOptions & { port: number; host?: string },
  onconnect?: () => void
): TLSSocket

export function createConnection(port: number, host?: string, onconnect?: () => void): TLSSocket

export { createConnection as connect }
