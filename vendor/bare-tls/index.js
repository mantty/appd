const { Duplex, Writable } = require('bare-stream')
const binding = require('./binding')
const constants = require('./lib/constants')
const errors = require('./lib/errors')

const defaultReadBufferSize = 65536

const context = binding.context()

exports.Socket = class TLSSocket extends Duplex {
  constructor(socket, opts = {}) {
    const {
      isServer = false,
      cert = null,
      key = null,
      host = null,
      rejectUnauthorized = true,
      ca = null,
      alpnProtocols = null,
      eagerOpen = true,
      allowHalfOpen = true,
      readBufferSize = defaultReadBufferSize
    } = opts

    if (!isServer && rejectUnauthorized && !host) {
      throw new TypeError("'host' is required for client sockets when 'rejectUnauthorized' is true")
    }

    super({ eagerOpen })

    this._state = 0

    this._socket = socket
    this._allowHalfOpen = allowHalfOpen

    this._pendingOpen = null
    this._pendingWrite = null
    this._pendingDestroy = null

    this._reading = Buffer.alloc(readBufferSize)
    this._buffer = []
    this._buffered = 0

    this._ondata = this._ondata.bind(this)
    this._ondrain = this._ondrain.bind(this)
    this._onend = this._onend.bind(this)
    this._onclose = this._onclose.bind(this)
    this._onerror = this._onerror.bind(this)

    socket.on('close', this._onclose).on('error', this._onerror)

    let alpn = null

    if (alpnProtocols && alpnProtocols.length > 0) {
      const parts = []

      for (const protocol of alpnProtocols) {
        const encoded = Buffer.from(protocol)

        if (encoded.byteLength === 0 || encoded.byteLength > 255) {
          throw new RangeError('ALPN protocol name must be 1-255 bytes')
        }

        parts.push(Buffer.of(encoded.byteLength), encoded)
      }

      alpn = Buffer.concat(parts)
    }

    try {
      this._handle = binding.init(
        context,
        isServer,
        cert,
        key,
        host,
        rejectUnauthorized,
        ca,
        alpn,
        this,
        this._onread,
        this._onwrite
      )
    } catch (err) {
      this._handle = null

      this.destroy(err)
    }
  }

  get socket() {
    return this._socket
  }

  get encrypted() {
    return true
  }

  get alpnProtocol() {
    return this._handle ? binding.alpnProtocol(this._handle) : null
  }

  _onconnect() {
    this._state |= constants.state.CONNECTED

    this.emit('connect')

    const cb = this._pendingOpen
    this._pendingOpen = null
    cb(null)
  }

  _ondata(data) {
    this._buffer.push(data)
    this._buffered += data.byteLength

    while (this._buffered > 0) {
      if (this._state & constants.state.CONNECTED) {
        let read
        try {
          read = binding.read(this._handle, this._reading)
        } catch (err) {
          return this.destroy(errors.from(err))
        }

        if (read < 0) break

        if (read === 0) {
          this.push(null)
          if (this._allowHalfOpen === false) this.end()
          return
        }

        const copy = Buffer.allocUnsafe(read)
        copy.set(this._reading.subarray(0, read))

        this.push(copy)
      } else {
        try {
          if (binding.handshake(this._handle)) this._onconnect()
          else break
        } catch (err) {
          err = errors.from(err)

          // Defer so any alert the handshake wrote to the underlying socket
          // gets a chance to flush before we tear it down.
          queueMicrotask(() => {
            if (this._pendingOpen) {
              const cb = this._pendingOpen
              this._pendingOpen = null
              cb(err)
            } else {
              this.destroy(err)
            }
          })
          return
        }
      }
    }
  }

  _ondrain() {
    if (this._pendingWrite === null) return
    const cb = this._pendingWrite
    this._pendingWrite = null
    cb(null)
  }

  _onend() {
    this.push(null)
  }

  _onclose() {
    this._socket.off('close', this._onclose).off('error', this._onerror)

    if (this._pendingDestroy) {
      const cb = this._pendingDestroy
      this._pendingDestroy = null
      cb()
    }
  }

  _onerror(err) {
    if (this._pendingOpen) {
      const cb = this._pendingOpen
      this._pendingOpen = null
      cb(err)
    } else if (this._pendingDestroy) {
      const cb = this._pendingDestroy
      this._pendingDestroy = null
      cb(err)
    } else {
      this.destroy(err)
    }
  }

  _onread(data) {
    if (this._buffered < data.byteLength) return 0

    let offset = 0
    let remaining = data.byteLength

    while (remaining > 0) {
      const chunk = this._buffer[0]

      if (chunk.byteLength <= remaining) {
        data.set(chunk, offset)

        offset += chunk.byteLength
        remaining -= chunk.byteLength

        this._buffer.shift()
      } else {
        data.set(chunk.subarray(0, remaining), offset)

        this._buffer[0] = chunk.subarray(remaining)

        remaining = 0
      }
    }

    this._buffered -= data.byteLength

    return data.byteLength
  }

  _onwrite(data) {
    this._socket.write(Buffer.from(data.slice()))

    return data.byteLength
  }

  _attach() {
    if (this._state & constants.state.ATTACHED) return
    this._state |= constants.state.ATTACHED

    this._socket.on('data', this._ondata).on('drain', this._ondrain).on('end', this._onend)
  }

  _detach() {
    if (!(this._state & constants.state.ATTACHED)) return
    this._state &= ~constants.state.ATTACHED

    this._socket.off('data', this._ondata).off('drain', this._ondrain).off('end', this._onend)
  }

  _open(cb) {
    this._pendingOpen = cb
    this._attach()

    try {
      if (binding.handshake(this._handle)) this._onconnect()
    } catch (err) {
      this._pendingOpen = null

      cb(errors.from(err))
    }
  }

  _write(data, encoding, cb) {
    try {
      binding.write(this._handle, data)

      if (Writable.isBackpressured(this._socket)) {
        this._pendingWrite = cb
      } else {
        cb(null)
      }
    } catch (err) {
      this._pendingWrite = null

      cb(errors.from(err))
    }
  }

  _final(cb) {
    try {
      binding.shutdown(this._handle)

      cb(null)
    } catch (err) {
      cb(err)
    }

    this._socket.end()
  }

  _predestroy() {
    if (this._handle) {
      this._detach()

      binding.destroy(this._handle)
      this._handle = null
    }

    this._socket.destroy()
  }

  _destroy(err, cb) {
    if (this._handle) {
      this._detach()

      binding.destroy(this._handle)
      this._handle = null
    }

    if (err) this._socket.destroy()

    if (this._socket.destroyed) cb(err)
    else this._pendingDestroy = cb
  }
}

exports.TLSSocket = exports.Socket // For Node.js compatibility

exports.constants = constants
exports.errors = errors

const net = require('./net')

exports.createConnection = net.createConnection
exports.createServer = net.createServer

// For Node.js compatibility
exports.connect = exports.createConnection
