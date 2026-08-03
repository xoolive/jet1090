use ansi_term::Color;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Result};

#[derive(Debug, Serialize, Deserialize)]
pub struct Airport {
    pub icao: String,
    pub iata: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub city: String,
    #[serde(rename = "countryCode")]
    pub country: String,
}

impl Display for Airport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result {
        write!(f, "{}", Color::RGB(0, 128, 0).bold().paint(&self.icao))?;
        write!(f, " - ")?;
        write!(f, "{}", Color::RGB(255, 69, 0).paint(&self.iata))?;
        write!(f, "    {:.3} {:.3}\t", self.lat, self.lon)?;
        write!(f, "{} ({})", self.name, self.country)
    }
}

const AIRPORTS_JSON: &str = include_str!("../../data/airports.json");
pub static AIRPORTS: Lazy<Vec<Airport>> =
    Lazy::new(|| serde_json::from_str(AIRPORTS_JSON).unwrap());

pub fn one_airport(args: &[Regex]) -> Option<&Airport> {
    'airport: for airport in AIRPORTS.iter() {
        for arg in args {
            if !arg.is_match(&airport.country)
                & !arg.is_match(&airport.icao)
                & !arg.is_match(&airport.iata)
                & !arg.is_match(&airport.city)
                & !arg.is_match(&airport.name)
            {
                continue 'airport;
            }
        }
        return Some(airport);
    }
    None
}
