use regex::Regex;
use rs1090::data::airports::AIRPORTS;

pub fn main() {
    let args: Vec<Regex> = std::env::args()
        .skip(1)
        .map(|a| Regex::new(&format!("(?i){}", a)).unwrap())
        .collect();

    'airport: for airport in AIRPORTS.iter() {
        for arg in &args {
            if !arg.is_match(&airport.country)
                & !arg.is_match(&airport.icao)
                & !arg.is_match(&airport.iata)
                & !arg.is_match(&airport.name)
            {
                continue 'airport;
            }
        }
        println!("{airport}");
    }
}
