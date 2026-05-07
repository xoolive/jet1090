use deku::prelude::*;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

#[cfg(feature = "bds-infer")]
use crate::data::patterns::PATTERNS;

/**
 * ## Aircraft and Airline Registration Markings (BDS 2,1)
 *
 * Comm-B message providing aircraft and airline registration information.  
 * Per ICAO Doc 9871 Table A-2-33: BDS code 2,1 — Aircraft and airline registration markings
 *
 * Purpose: To permit ground systems to identify the aircraft without the
 * necessity of compiling and maintaining continuously updated data banks.
 *
 * Message Structure (56 bits):
 * | AC_STAT | AC_REG (7 chars) | AL_STAT | AL_REG (2 chars) |
 * |---------|------------------|---------|------------------|
 * | 1       | 42 (6×7)         | 1       | 12 (6×2)         |
 *
 * Field Encoding per ICAO Doc 9871:
 *
 * **Aircraft Registration Status** (bit 1):
 *   - 0 = aircraft registration not available or invalid
 *   - 1 = aircraft registration available and valid
 *
 * **Aircraft Registration Number** (bits 2-43): 7 characters, 6 bits each
 *   - Character encoding per ICAO Annex 10, Vol IV, Table 3-7
 *   - Valid characters: A-Z (1-26), 0-9 (48-57), # (0), space (32)
 *   - Example formats: "N12345", "GABCD", "VHVKI" (national dash stripped)
 *   - Must match pattern: [A-Z0-9]+[\\s#]?[A-Z0-9]+
 *
 * **Airline Registration Status** (bit 44):
 *   - 0 = airline designation not available or invalid
 *   - 1 = airline designation available and valid
 *
 * **ICAO Airline Registration Marking** (bits 45-56): 2 characters, 6 bits each
 *   - Character encoding per ICAO Annex 10, Vol IV, Table 3-7
 *   - Valid characters: A-Z (1-26), 0-9 (48-57)
 *   - Note: Most transponders don't implement this field (status=0)
 *
 * Character Set (6-bit encoding):
 * - 0 (000000) = # (no character marker)
 * - 1-26 (000001-011010) = A-Z
 * - 32 (100000) = space
 * - 48-57 (110000-111001) = 0-9
 *
 * Validation Rules:
 * - If status bit is 0, all character bits must be 0
 * - If status bit is 1, characters must form valid registration
 * - Aircraft registration must match expected national format
 * - Airline designation rarely implemented in practice
 *
 * Note: This provides aircraft tail number/registration separate from
 * callsign (BDS 2,0/0,8), allowing ground systems to identify aircraft
 * without maintaining extensive databases.
 *
 * Reference: ICAO Doc 9871 Table A-2-33, Annex 10 Vol IV Table 3-7
 */

#[derive(
    Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone, Default,
)]
#[serde(tag = "bds", rename = "21")]
pub struct AircraftAndAirlineRegistrationMarkings {
    #[deku(bits = "1")]
    #[serde(skip)]
    /// Aircraft Registration Status
    pub ac_status: bool,

    #[deku(reader = "aircraft_registration_read(deku::reader, *ac_status)")]
    #[serde(rename = "registration")]
    /// Aircraft Registration Number (7 characters)
    pub aircraft_registration: Option<String>,

    #[deku(bits = "1")]
    #[serde(skip)]
    /// Airline Registration Status
    pub al_status: bool,

    #[deku(reader = "airline_registration_read(deku::reader, *al_status)")]
    #[serde(rename = "airline", skip_serializing_if = "Option::is_none")]
    /// ICAO Airline Registration Marking (2 characters)
    pub airline_registration: Option<String>,
}

// Per ICAO Annex 10 Vol IV Table 3-7, the 6-bit BDS 2,1 alphabet only defines
// A–Z (1–26), SPACE (32), and 0–9 (48–57). There is **no hyphen character**
// in the alphabet. Real registrations such as "B-2487" must be transmitted
// without the dash; the avionics use one of three placeholder values for
// the dash slot, and we treat all three the same way (strip them):
//
//   * 32 (100000)  — SPACE: the standards-conformant placeholder.
//   * 0  (000000)  — NULL:  used by some implementations; not in the alphabet
//                    but interpreted here as an empty character (≡ space).
//   * 45 (101101)  — dash placeholder observed empirically in our Jan 2025
//                    dataset (B#7838, VH#8MR, XA#ADD, ...). Code 45 is in
//                    the reserved range (33–47) but is used consistently by
//                    multiple avionics vendors as the national dash. Treated
//                    as a placeholder rather than noise.
//
// Other reserved code points (27–31, 33–44, 46–47, 58–63) are kept as '#'
// so that genuinely-corrupt or out-of-band payloads (e.g. random BDS 4,0
// bits decoded as BDS 2,1 character noise) remain visible to downstream.
const CHAR_LOOKUP: &[u8; 64] =
    b" ABCDEFGHIJKLMNOPQRSTUVWXYZ##### ############-##0123456789######";

/// Returns true when this 6-bit code is a national-dash placeholder that
/// should be stripped from the decoded registration string.
#[inline]
fn is_dash_placeholder(c: u8) -> bool {
    c == 0 || c == 32 || c == 45
}

pub fn aircraft_registration_read<
    R: deku::no_std_io::Read + deku::no_std_io::Seek,
>(
    reader: &mut Reader<R>,
    status: bool,
) -> Result<Option<String>, DekuError> {
    let mut chars = vec![];
    for _ in 0..=6 {
        let c = u8::from_reader_with_ctx(reader, deku::ctx::BitSize(6))?;
        trace!("Reading letter {}", CHAR_LOOKUP[c as usize] as char);
        // Strip national-dash placeholders (space / null / 45) so the decoded
        // string is the dash-less registration (e.g. "B 2487", "B\x002487"
        // and "B-2487" all decode to "B2487"). Downstream callers can re-insert
        // the dash from the icao24 range (see patterns.json).
        if !is_dash_placeholder(c) {
            chars.push(c);
        }
    }

    let all_zeros = chars.is_empty();
    let encoded = chars
        .into_iter()
        .map(|b| CHAR_LOOKUP[b as usize] as char)
        .collect::<String>();
    debug!("Decoded registration: {}", encoded);

    if status {
        // hard-reject: real BDS 21 registrations contain at most 2 `#`
        // placeholders (used by the 6-bit ICAO alphabet for non-alphanumeric
        // positions). Strings with 3 or more `#` are almost always phantom
        // decodes from a different BDS payload. The stricter
        // country-prefix pattern check belongs to a separate filtering
        // stage and is not enforced here.
        #[cfg(feature = "bds-infer")]
        if encoded.chars().filter(|&c| c == '#').count() > 2 {
            return Err(DekuError::Assertion(
                format!(
                    "BDS 21 registration {encoded:?} has > 2 '#' placeholders"
                )
                .into(),
            ));
        }
        Ok(Some(encoded))
    } else if all_zeros {
        Ok(None)
    } else {
        Err(DekuError::Assertion(
            format!(
                "Non-null value after invalid aircraft registration status: {encoded}"
            )
            .into(),
        ))
    }
}

pub fn airline_registration_read<
    R: deku::no_std_io::Read + deku::no_std_io::Seek,
>(
    reader: &mut Reader<R>,
    status: bool,
) -> Result<Option<String>, DekuError> {
    let mut chars = vec![];
    for _ in 0..2 {
        let c = u8::from_reader_with_ctx(reader, deku::ctx::BitSize(6))?;
        trace!("Reading letter {}", CHAR_LOOKUP[c as usize] as char);
        if !is_dash_placeholder(c) {
            chars.push(c);
        }
    }
    let all_zeros = chars.is_empty();
    let encoded = chars
        .into_iter()
        .map(|b| CHAR_LOOKUP[b as usize] as char)
        .collect::<String>();

    if status {
        // Ok((inside_rest, Some(encoded)))
        Err(DekuError::Assertion(
            format!(
                "Most transponders don't implement this field. (value = {encoded})"
            )
            .into(),
        ))
    } else if all_zeros {
        Ok(None)
    } else {
        Err(DekuError::Assertion(
            format!(
                "Non-null value after invalid airline registration status: {encoded}"
            )
            .into(),
        ))
    }
}

/// Validate a decoded BDS 2,1 registration against the country-prefix
/// patterns derived from the aircraft's `icao24` address.
///
/// Returns `true` when the registration is plausible for the country
/// inferred from the ICAO 24-bit address range, `false` when it should
/// be rejected as a phantom candidate.
///
/// The check is organised into four buckets, tried in order:
///
/// 1. **Direct country-prefix match** — the registration matches the
///    country's own pattern (e.g. `JA840J` for Japan `0x84xxxx`).
/// 2. **Airline-prefix strip** — drop 2–3 leading characters (IATA airline
///    code) and retry bucket 1 (e.g. `SQ9VDHA` → `9V-DHA` Singapore).
/// 3. **Airline-suffix strip** — drop 2 trailing characters and retry
///    bucket 1 (e.g. `B2487CA` → `B-2487` China).
/// 4. **Flight-callsign shape** — matches `^[A-Z]{2,4}\d{2,5}[A-Z]{0,3}$`
///    with length 5–7 and no `#` (e.g. `BRB1672`, `CIB1800`).
///
/// When no country pattern can be resolved for the `icao24` (unknown range
/// or pattern-less entry), the function returns `true` to avoid false
/// rejects on legitimate but uncommon registrations.
#[cfg(feature = "bds-infer")]
pub fn validate_registration(reg: &str, icao24: u32) -> bool {
    // A leading placeholder is not a plausible registration prefix. Keeping
    // such strings would be especially dangerous because BDS 2,1 is later
    // treated as a high-confidence winner. Example false positive observed in
    // Comm-B payloads: "#0NF" for a French address.
    if reg.starts_with('#') {
        return false;
    }

    // Bucket 4: flight-callsign shape — checked first because it is
    // icao24-independent and very cheap.
    static CALLSIGN_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^[A-Z]{2,4}\d{2,5}[A-Z]{0,3}$").unwrap());

    // Resolve the country pattern for this ICAO address range.
    let country_pattern: Option<String> = PATTERNS
        .registers
        .iter()
        .find(|r| {
            if let (Some(s), Some(e)) = (&r.start, &r.end) {
                let start =
                    u32::from_str_radix(&s[2..], 16).unwrap_or(u32::MAX);
                let end = u32::from_str_radix(&e[2..], 16).unwrap_or(0);
                icao24 >= start && icao24 <= end
            } else {
                false
            }
        })
        .and_then(|r| r.pattern.clone());

    // If no pattern is found for this range, accept unconditionally.
    let Some(raw_pattern) = country_pattern else {
        return true;
    };

    // Build a dash-stripped version of the pattern so it matches our
    // canonical (dash-less) registrations.
    let stripped_pattern = raw_pattern.replace('-', "");
    let Ok(re) = Regex::new(&stripped_pattern) else {
        return true; // malformed pattern → accept
    };

    // Helper for country-prefix matches. Most country patterns are prefixes
    // such as "^F-" (France), which become "^F" after dash stripping. In that
    // case, matching the prefix alone is not enough: a registration must have
    // at least one character after the country prefix. More specific patterns
    // containing digit constraints (e.g. "^B-\\d{5}") are allowed to consume
    // the whole candidate.
    let country_match = |candidate: &str| -> bool {
        let Some(m) = re.find(candidate) else {
            return false;
        };
        if m.start() != 0 {
            return false;
        }
        stripped_pattern.contains("\\d") || candidate.len() > m.end()
    };

    // Bucket 1: direct country-prefix match.
    if country_match(reg) {
        return true;
    }

    // Buckets 2 & 3: airline-prefix/suffix strip.
    for strip_len in [2usize, 3] {
        // Prefix strip
        if reg.len() > strip_len && country_match(&reg[strip_len..]) {
            return true;
        }
        // Suffix strip (only length 2)
        if strip_len == 2 && reg.len() > strip_len {
            let end = reg.len() - strip_len;
            if country_match(&reg[..end]) {
                return true;
            }
        }
    }

    // Bucket 4: callsign shape (no `#`, length 5–7, IATA-like prefix).
    if reg.len() >= 5
        && reg.len() <= 7
        && !reg.contains('#')
        && CALLSIGN_RE.is_match(reg)
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use hexlit::hex;

    #[test]
    fn test_valid_bds21() {
        let bytes = hex!("a00002bf940f19680c0000000000");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBAltitudeReply { bds, .. } = msg.df {
            let AircraftAndAirlineRegistrationMarkings {
                aircraft_registration,
                ..
            } = bds.bds21.unwrap();
            assert_eq!(aircraft_registration, Some("JA824A".to_string()));
        } else {
            unreachable!();
        }

        let bytes = hex!("a00002988230c3b470a000000000");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBAltitudeReply { bds, .. } = msg.df {
            let AircraftAndAirlineRegistrationMarkings {
                aircraft_registration,
                ..
            } = bds.bds21.unwrap();
            assert_eq!(aircraft_registration, Some("AFFGZNE".to_string()));
        } else {
            unreachable!();
        }
        let bytes = hex!("a0000793ac45ab164c0000000000");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBAltitudeReply { bds, .. } = msg.df {
            let AircraftAndAirlineRegistrationMarkings {
                aircraft_registration,
                ..
            } = bds.bds21.unwrap();
            assert_eq!(aircraft_registration, Some("VHVKI".to_string()));
        } else {
            unreachable!();
        }
    }

    #[cfg(feature = "bds-infer")]
    #[test]
    fn test_validate_registration() {
        // Bucket 1: direct country match
        // Japan: icao24 0x84xxxx, pattern "^JA"
        assert!(validate_registration("JA824A", 0x843a1b));
        // US: icao24 0xa0xxxx, pattern "^N"
        assert!(validate_registration("N706CK", 0xa4a6fd));
        // China: icao24 0x78xxxx, pattern "^B-"
        assert!(validate_registration("B2487", 0x780123));
        // France: icao24 0x38xxxx, pattern "^F-"
        assert!(validate_registration("FGZHA", 0x3950ab));

        // Bucket 2: airline-prefix strip (3 chars: SQ + 9V-DHA -> 9VDHA)
        // Singapore: icao24 0x76xxxx, pattern "^9V-"
        assert!(validate_registration("SQ9VDHA", 0x76c123));

        // Bucket 3: airline-suffix strip (2 chars: B2487CA -> B2487)
        assert!(validate_registration("B2487CA", 0x780123));

        // Bucket 4: callsign shape
        assert!(validate_registration("BRB1672", 0x843a1b));
        assert!(validate_registration("CIB1800", 0x843a1b));

        // Reject: leading placeholder and bare country prefix are too weak
        // to be considered plausible registrations.
        assert!(!validate_registration("#0NF", 0x39c422));
        assert!(!validate_registration("F", 0x39c422));

        // Reject: clearly wrong country prefix for icao24 range
        // Japan range, non-JA
        // Reject: all-hash (would be caught by the '#' count check upstream,
        // but validate_registration also rejects non-matching strings)
        assert!(!validate_registration("XXXXXX", 0x843a1b));

        assert!(!validate_registration("ZZZZZZ", 0x843a1b)); // Japan range, non-JA
    }
}
