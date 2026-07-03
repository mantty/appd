type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn round_trips_opaque_payload_through_capnp_message() -> TestResult {
    let payload = b"appd capnp boundary";

    let encoded = appd_runtime::capnp_support::encode_payload(payload)?;
    let decoded = appd_runtime::capnp_support::decode_payload(&encoded)?;

    assert_eq!(decoded, payload);
    Ok(())
}
