//! Outbound-HTTP smoke-test guest (phase 2e).
//!
//! Reads the target authority (host:port) the host injects via the
//! `HTTP_TARGET_AUTHORITY` environment variable, issues a plain `GET /` over
//! `wasi:http/outgoing-handler`, and asserts the response is `200` with body
//! `lunatic-http-ok`. Any deviation panics, which traps the guest and fails the
//! host test. Importing `wasi:http` here is what makes the component fail to
//! instantiate when the host withholds the HTTP surface (the deny path).

use wasi::http::outgoing_handler;
use wasi::http::types::{Fields, Method, OutgoingRequest, Scheme};

fn main() {
    let authority =
        std::env::var("HTTP_TARGET_AUTHORITY").expect("host injects HTTP_TARGET_AUTHORITY");

    let request = OutgoingRequest::new(Fields::new());
    request.set_method(&Method::Get).expect("set method");
    request.set_scheme(Some(&Scheme::Http)).expect("set scheme");
    request
        .set_authority(Some(&authority))
        .expect("set authority");
    request
        .set_path_with_query(Some("/"))
        .expect("set path");

    let future = outgoing_handler::handle(request, None).expect("outbound handle accepted");
    // Block until the response headers are available.
    future.subscribe().block();
    let response = future
        .get()
        .expect("response is ready after blocking")
        .expect("response future not already taken")
        .expect("outbound request succeeded");

    let status = response.status();
    assert_eq!(status, 200, "expected HTTP 200, got {status}");

    // Drain the response body.
    let body = response.consume().expect("consume response body");
    let stream = body.stream().expect("open body stream");
    let mut buf = Vec::new();
    loop {
        match stream.blocking_read(4096) {
            Ok(chunk) if chunk.is_empty() => break,
            Ok(chunk) => buf.extend_from_slice(&chunk),
            // `StreamError::Closed` signals end-of-body.
            Err(_) => break,
        }
    }

    let text = String::from_utf8(buf).expect("body is utf-8");
    assert_eq!(text, "lunatic-http-ok", "unexpected response body: {text:?}");
    println!("outbound-http guest received: {text}");
}
