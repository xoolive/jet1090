#![allow(rustdoc::broken_intra_doc_links)]
#![allow(clippy::needless_doctest_main)]
#![doc = include_str!("../readme.md")]
pub mod data;
pub mod decode;
pub mod source;

pub mod prelude {
    /// This re-export is necessary to decode messages
    pub use deku::prelude::*;

    pub use crate::decode::adsb::{ADSB, ME};
    pub use crate::decode::bds::bds05::AirbornePosition;
    pub use crate::decode::bds::bds06::SurfacePosition;
    pub use crate::decode::bds::bds08::AircraftIdentification;
    pub use crate::decode::bds::bds09::AirborneVelocity;
    pub use crate::decode::bds::bds61::AircraftStatus;
    pub use crate::decode::bds::bds62::TargetStateAndStatusInformation;
    pub use crate::decode::bds::bds65::AircraftOperationStatus;
    /// The root structure to decode messages
    pub use crate::decode::Message;
    pub use crate::decode::DF::*;
    pub use crate::decode::{
        cpr::Position, SensorMetadata, TimedMessage, ICAO,
    };

    /// This re-export is necessary for the following export
    pub use futures_util::stream::StreamExt;

    /// Information on the structure of a Beast message
    #[cfg(not(target_arch = "wasm32"))]
    pub use crate::source::beast;

    pub use crate::source::iqread;

    #[cfg(feature = "sero")]
    pub use crate::source::sero;
}
