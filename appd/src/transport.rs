use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::Receiver;

use base64::{Engine, engine::general_purpose::STANDARD};
use openssl::sha::sha1;
use openssl::ssl::{SslAcceptor, SslMethod, SslStream, SslVerifyMode};
use openssl::x509::X509;
use serde::Serialize;

use crate::gateway::{WebSocketBridge, WebSocketInbound, WebSocketOutbound};
use crate::quickjs::{Error, RuntimeConfig};

const MAX_HEADERS: usize = 64 * 1024;
const MAX_BODY: usize = 16 * 1024 * 1024;

#[derive(Serialize)]
pub(super) struct HttpRequest {
    pub(super) method: String,
    pub(super) target: String,
    pub(super) url: String,
    pub(super) headers: BTreeMap<String, String>,
    pub(super) body: Option<String>,
}

pub(super) struct HttpResponse {
    pub(super) status: u16,
    pub(super) headers: BTreeMap<String, String>,
    pub(super) body: Vec<u8>,
}

pub(super) struct WebSocketFrame {
    pub(super) final_frame: bool,
    pub(super) opcode: u8,
    pub(super) payload: Vec<u8>,
}

impl HttpResponse {
    pub(super) fn text(status: u16, body: &str) -> Self {
        let mut headers = BTreeMap::new();
        headers.insert(
            "content-type".to_owned(),
            "text/plain; charset=utf-8".to_owned(),
        );
        Self {
            status,
            headers,
            body: body.as_bytes().to_vec(),
        }
    }
}

pub(super) fn read_headers(stream: &mut TcpStream) -> Result<Vec<u8>, Error> {
    read_header_block(stream)
}

fn read_header_block(stream: &mut impl Read) -> Result<Vec<u8>, Error> {
    let mut data = Vec::new();
    loop {
        let mut byte = [0; 1];
        stream.read_exact(&mut byte)?;
        data.push(byte[0]);
        if data.len() > MAX_HEADERS {
            return Err(Error::Startup("HTTP headers exceed the limit".to_owned()));
        }
        if data.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    Ok(data)
}

pub(super) fn is_connect(data: &[u8], host: &str) -> bool {
    let text = String::from_utf8_lossy(data);
    let line = text.lines().next().unwrap_or_default();
    let mut parts = line.split_whitespace();
    parts.next() == Some("CONNECT")
        && parts
            .next()
            .is_some_and(|target| target.eq_ignore_ascii_case(&format!("{host}:443")))
}

pub(super) fn tls_acceptor(config: &RuntimeConfig) -> Result<SslAcceptor, Error> {
    let mut builder = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls())
        .map_err(|error| tls_error(&error))?;
    let certificate = X509::from_pem(&std::fs::read(&config.certificates.certificate)?)
        .map_err(|error| tls_error(&error))?;
    let private_key = openssl::pkey::PKey::private_key_from_pem(&std::fs::read(
        &config.certificates.private_key,
    )?)
    .map_err(|error| tls_error(&error))?;
    builder
        .set_certificate(&certificate)
        .map_err(|error| tls_error(&error))?;
    builder
        .set_private_key(&private_key)
        .map_err(|error| tls_error(&error))?;
    builder
        .check_private_key()
        .map_err(|error| tls_error(&error))?;
    if config.require_client_certificate {
        let ca = X509::from_pem(&std::fs::read(&config.certificates.ca)?)
            .map_err(|error| tls_error(&error))?;
        builder
            .cert_store_mut()
            .add_cert(ca)
            .map_err(|error| tls_error(&error))?;
        builder.set_verify(SslVerifyMode::PEER);
    }
    Ok(builder.build())
}

pub(super) fn read_request(
    stream: &mut SslStream<TcpStream>,
    host: &str,
) -> Result<HttpRequest, Error> {
    let headers = read_header_block(stream)?;
    let text = String::from_utf8_lossy(&headers);
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_owned();
    let target = request_parts.next().unwrap_or("/").to_owned();
    if method.is_empty() {
        return Err(Error::Startup("HTTP method is missing".to_owned()));
    }
    let mut request_headers = BTreeMap::new();
    let mut content_length = 0usize;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_owned();
        if name == "content-length" {
            content_length = value
                .parse()
                .map_err(|_| Error::Startup("invalid content length".to_owned()))?;
        }
        request_headers.insert(name, value);
    }
    if content_length > MAX_BODY {
        return Err(Error::Startup("HTTP body exceeds the limit".to_owned()));
    }
    let mut body = vec![0; content_length];
    stream.read_exact(&mut body)?;
    let body = if body.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&body).into_owned())
    };
    Ok(HttpRequest {
        method,
        target: target.clone(),
        url: format!("https://{host}{target}"),
        headers: request_headers,
        body,
    })
}

pub(super) fn is_websocket(request: &HttpRequest) -> bool {
    request
        .headers
        .get("upgrade")
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
}

pub(super) fn websocket_session(
    stream: &mut SslStream<TcpStream>,
    key: Option<&str>,
    bridge: &WebSocketBridge,
) -> Result<(), Error> {
    let key = key.ok_or_else(|| Error::Startup("WebSocket key is missing".to_owned()))?;
    let digest = sha1(format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes());
    let accept = STANDARD.encode(digest);
    write!(
        stream,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    )?;
    stream.flush()?;
    if flush_websocket_outbound(stream, &bridge.outgoing)? {
        return Ok(());
    }

    let mut fragmented: Option<(u8, Vec<u8>)> = None;
    loop {
        let Some(frame) = read_websocket_frame(stream)? else {
            return Ok(());
        };
        match frame.opcode {
            0x8 => {
                let (code, reason) = websocket_close(&frame.payload)?;
                write_websocket_frame(stream, frame.opcode, &frame.payload)?;
                let _ = bridge
                    .incoming
                    .send(WebSocketInbound::Close { code, reason });
                return Ok(());
            }
            0x9 => write_websocket_frame(stream, 0xA, &frame.payload)?,
            0xA => {}
            0x0..=0x2 => {
                queue_websocket_message(
                    bridge,
                    &mut fragmented,
                    frame.final_frame,
                    frame.opcode,
                    frame.payload,
                )?;
                if frame.final_frame && flush_websocket_outbound(stream, &bridge.outgoing)? {
                    return Ok(());
                }
            }
            _ => return Err(Error::Startup("invalid WebSocket opcode".to_owned())),
        }
    }
}

fn read_websocket_frame(
    stream: &mut SslStream<TcpStream>,
) -> Result<Option<WebSocketFrame>, Error> {
    let mut header = [0; 2];
    if stream.read_exact(&mut header).is_err() {
        return Ok(None);
    }
    let final_frame = header[0] & 0x80 != 0;
    let opcode = header[0] & 0x0f;
    if header[0] & 0x70 != 0 {
        return Err(Error::Startup(
            "WebSocket reserved bits are unsupported".to_owned(),
        ));
    }
    if header[1] & 0x80 == 0 {
        return Err(Error::Startup(
            "client WebSocket frames must be masked".to_owned(),
        ));
    }
    let mut length = usize::from(header[1] & 0x7f);
    if length == 126 {
        let mut value = [0; 2];
        stream.read_exact(&mut value)?;
        length = usize::from(u16::from_be_bytes(value));
    } else if length == 127 {
        let mut value = [0; 8];
        stream.read_exact(&mut value)?;
        let length64 = u64::from_be_bytes(value);
        length = usize::try_from(length64)
            .map_err(|_| Error::Startup("WebSocket frame is too large".to_owned()))?;
    }
    if length > MAX_BODY {
        return Err(Error::Startup("WebSocket frame is too large".to_owned()));
    }
    if opcode >= 0x8 && (!final_frame || length > 125) {
        return Err(Error::Startup(
            "WebSocket control frame is invalid".to_owned(),
        ));
    }
    let mut mask = [0; 4];
    stream.read_exact(&mut mask)?;
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload)?;
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[index % mask.len()];
    }
    Ok(Some(WebSocketFrame {
        final_frame,
        opcode,
        payload,
    }))
}

pub(super) fn queue_websocket_message(
    bridge: &WebSocketBridge,
    fragmented: &mut Option<(u8, Vec<u8>)>,
    final_frame: bool,
    opcode: u8,
    payload: Vec<u8>,
) -> Result<(), Error> {
    let (message_opcode, payload) = match opcode {
        0x0 => {
            let Some((initial_opcode, mut message)) = fragmented.take() else {
                return Err(Error::Startup(
                    "WebSocket continuation has no initial frame".to_owned(),
                ));
            };
            if message.len().saturating_add(payload.len()) > MAX_BODY {
                return Err(Error::Startup("WebSocket message is too large".to_owned()));
            }
            message.extend_from_slice(&payload);
            (initial_opcode, message)
        }
        0x1 | 0x2 => {
            if fragmented.is_some() {
                return Err(Error::Startup(
                    "WebSocket message starts before the previous message ended".to_owned(),
                ));
            }
            (opcode, payload)
        }
        _ => return Err(Error::Startup("invalid WebSocket opcode".to_owned())),
    };
    if final_frame {
        bridge
            .incoming
            .send(WebSocketInbound::Message {
                binary: message_opcode == 0x2,
                payload,
            })
            .map_err(|_| Error::Startup("WebSocket worker closed".to_owned()))?;
    } else {
        *fragmented = Some((message_opcode, payload));
    }
    Ok(())
}

fn flush_websocket_outbound(
    stream: &mut SslStream<TcpStream>,
    outgoing: &Receiver<WebSocketOutbound>,
) -> Result<bool, Error> {
    while let Ok(frame) = outgoing.recv() {
        match frame {
            WebSocketOutbound::Message { binary, payload } => {
                write_websocket_frame(stream, if binary { 0x2 } else { 0x1 }, &payload)?;
            }
            WebSocketOutbound::Close { code, reason } => {
                let payload = websocket_close_payload(code, &reason)?;
                write_websocket_frame(stream, 0x8, &payload)?;
                return Ok(true);
            }
            WebSocketOutbound::Ready => return Ok(false),
        }
    }
    Err(Error::Startup("WebSocket worker closed".to_owned()))
}

fn websocket_close(payload: &[u8]) -> Result<(u16, String), Error> {
    if payload.is_empty() {
        return Ok((1000, String::new()));
    }
    if payload.len() == 1 {
        return Err(Error::Startup(
            "WebSocket close payload is invalid".to_owned(),
        ));
    }
    let code = u16::from_be_bytes([payload[0], payload[1]]);
    if !valid_websocket_close_code(code) {
        return Err(Error::Startup("WebSocket close code is invalid".to_owned()));
    }
    let reason = std::str::from_utf8(&payload[2..])
        .map_err(|_| Error::Startup("WebSocket close reason is invalid".to_owned()))?
        .to_owned();
    Ok((code, reason))
}

fn valid_websocket_close_code(code: u16) -> bool {
    matches!(code, 1000..=1003 | 1007..=1014 | 3000..=4999)
}

fn websocket_close_payload(code: u16, reason: &str) -> Result<Vec<u8>, Error> {
    if !valid_websocket_close_code(code) {
        return Err(Error::Startup("WebSocket close code is invalid".to_owned()));
    }
    if reason.len() > 123 {
        return Err(Error::Startup(
            "WebSocket close reason is too long".to_owned(),
        ));
    }
    let mut payload = Vec::with_capacity(2 + reason.len());
    payload.extend_from_slice(&code.to_be_bytes());
    payload.extend_from_slice(reason.as_bytes());
    Ok(payload)
}

fn write_websocket_frame(
    stream: &mut SslStream<TcpStream>,
    opcode: u8,
    payload: &[u8],
) -> Result<(), Error> {
    let length = payload.len();
    stream.write_all(&[0x80 | opcode])?;
    if length <= 125 {
        stream.write_all(&[u8::try_from(length)
            .map_err(|_| Error::Startup("WebSocket frame length is invalid".to_owned()))?])?;
    } else if u16::try_from(length).is_ok() {
        stream.write_all(&[126])?;
        stream.write_all(
            &u16::try_from(length)
                .map_err(|_| Error::Startup("WebSocket frame length is invalid".to_owned()))?
                .to_be_bytes(),
        )?;
    } else {
        stream.write_all(&[127])?;
        stream.write_all(&(length as u64).to_be_bytes())?;
    }
    stream.write_all(payload)?;
    stream.flush()?;
    Ok(())
}

pub(super) fn write_plain_response(
    stream: &mut TcpStream,
    response: HttpResponse,
) -> Result<(), Error> {
    write_response_inner(stream, response)
}

pub(super) fn write_response(
    stream: &mut SslStream<TcpStream>,
    response: HttpResponse,
) -> Result<(), Error> {
    write_response_inner(stream, response)
}

fn write_response_inner(mut stream: impl Write, mut response: HttpResponse) -> Result<(), Error> {
    response
        .headers
        .entry("content-length".to_owned())
        .or_insert_with(|| response.body.len().to_string());
    response
        .headers
        .entry("connection".to_owned())
        .or_insert_with(|| "close".to_owned());
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "",
    };
    write!(stream, "HTTP/1.1 {} {reason}\r\n", response.status)?;
    for (name, value) in response.headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    stream.write_all(&response.body)?;
    stream.flush()?;
    Ok(())
}

fn tls_error(error: &openssl::error::ErrorStack) -> Error {
    Error::Tls(error.to_string())
}
