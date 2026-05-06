/// BDS subcommand — decode or infer BDS payloads
use crate::beast::bds_code_from_decoded;
use crate::error::{ErrorCode, ErrorResponse};
use rs1090::decode::bds::{decode_bds, infer_bds};

/// Decode BDS payload with Layer 1 API (decode_bds or infer_bds)
pub fn process_bds_decode(
    payload_hex: &str,
    code_filter: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Decode hex string to bytes
    let payload = hex::decode(payload_hex).inspect_err(|e| {
        let err = ErrorResponse::new(
            ErrorCode::InvalidHexPayload,
            format!("Invalid hex format: {}", e),
        );
        eprintln!("{}", err.to_json().unwrap());
    })?;

    // Verify payload is exactly 7 bytes (56 bits)
    if payload.len() != 7 {
        let err = ErrorResponse::with_context(
            ErrorCode::InvalidPayloadLength,
            "BDS payload must be exactly 7 bytes",
            serde_json::json!({
                "expected": 7,
                "received": payload.len()
            }),
        );
        eprintln!("{}", err.to_json()?);
        std::process::exit(1);
    }

    let results = if let Some(code_str) = code_filter {
        // Strict decoding with specified BDS code
        let bds_code = u8::from_str_radix(&code_str, 16).inspect_err(|e| {
            let err = ErrorResponse::new(
                ErrorCode::InvalidBdsCode,
                format!("Invalid BDS code '{}': {}", code_str, e),
            );
            eprintln!("{}", err.to_json().unwrap());
        })?;

        match decode_bds(&payload, bds_code) {
            Ok(decoded) => serde_json::json!({
                "payload": hex::encode(&payload),
                "bds": format!("{:02x}", bds_code),
                "decoded": serde_json::to_value(&decoded).unwrap_or(serde_json::json!(null))
            }),
            Err(e) => {
                let err = ErrorResponse::with_context(
                    ErrorCode::DecodingFailed,
                    format!("Failed to decode BDS {:02x}", bds_code),
                    serde_json::json!({ "detail": e.to_string() }),
                );
                eprintln!("{}", err.to_json()?);
                std::process::exit(1);
            }
        }
    } else {
        // Inference mode: try all BDS codes
        let inferred = infer_bds(&payload, None);
        let mut decoded_map = serde_json::json!({});

        for decoded in inferred.iter() {
            let bds_code = bds_code_from_decoded(decoded);
            decoded_map[format!("{:02x}", bds_code)] =
                serde_json::to_value(decoded)
                    .unwrap_or(serde_json::json!(null));
        }

        serde_json::json!({
            "payload": hex::encode(&payload),
            "mode": "infer",
            "decoded": decoded_map
        })
    };

    println!("{}", serde_json::to_string(&results)?);
    Ok(())
}
