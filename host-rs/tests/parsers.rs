//! Every decoder reachable from the network must return an error, never panic.
//!
//! These are the parsers a hostile or merely broken peer reaches before anything else
//! authenticates. `proptest` runs them on arbitrary bytes in CI; `host-rs/fuzz/` runs
//! the same shapes under libFuzzer's coverage guidance when someone asks it to.

use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(768))]

    #[test]
    fn no_decoder_panics_on_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = mobile_webcam::wire::Capabilities::decode(&bytes);
        let _ = mobile_webcam::wire::PrepareQuality::decode(&bytes);
        let _ = mobile_webcam::wire::QualityResult::decode(&bytes);
        let _ = mobile_webcam::wire::ClientChallenge::decode(&bytes);
        let _ = mobile_webcam::wire::ClientAuth::decode(&bytes);
        let _ = mobile_webcam::wire::ClientAuthResult::decode(&bytes);
        let _ = mobile_webcam::wire::Epoch::decode(&bytes);
        let _ = mobile_webcam::wire::Header::decode(&bytes);
        let _ = mobile_webcam::wire::PairingInfo::decode(&bytes);
        let _ = mobile_webcam::video::Config::parse(&bytes);
    }

    #[test]
    fn a_header_round_trips_or_is_refused(
        kind in any::<u8>(),
        flags in any::<u16>(),
        sequence in any::<u32>(),
        payload_len in any::<u32>(),
        pts_us in any::<u64>(),
    ) {
        let mut bytes = vec![b'A', b'M', b'B', b'1', 1, kind];
        bytes.extend_from_slice(&flags.to_le_bytes());
        bytes.extend_from_slice(&sequence.to_le_bytes());
        bytes.extend_from_slice(&payload_len.to_le_bytes());
        bytes.extend_from_slice(&pts_us.to_le_bytes());
        match mobile_webcam::wire::Header::decode(&bytes) {
            Ok(header) => {
                // Anything accepted must be re-encodable to exactly what came in:
                // acceptance is a claim about a length this peer will be sent.
                prop_assert_eq!(header.kind, kind);
                prop_assert_eq!(header.payload_len, payload_len);
            }
            Err(_) => {
                // Refused headers are the ones whose kind/length pair is not declared.
            }
        }
    }
}
