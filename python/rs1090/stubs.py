from __future__ import annotations

from typing import Literal, TypedDict, Union, overload

from typing_extensions import (
    NotRequired,  # python <3.11
    TypeGuard,  # python <3.10
)


class BDS05(TypedDict):
    bds: Literal["05"]
    NUCp: int
    NICb: int
    altitude: int
    source: str
    parity: Literal["odd", "even"]
    lat_cpr: int
    lon_cpr: int
    latitude: NotRequired[float]
    longitude: NotRequired[float]


class BDS10(TypedDict):
    bds: Literal["10"]
    config: bool
    ovc: bool
    acas: bool
    subnet: int
    level5: bool
    mode_s: bool
    identification: bool
    squitter: bool
    sic: bool
    gicb: bool
    acas_hybrid: bool
    acas_ra: bool
    dte: int


class BDS17(TypedDict):
    bds: Literal["17"]
    bds05: NotRequired[Literal[True]]
    bds06: NotRequired[Literal[True]]
    bds07: NotRequired[Literal[True]]
    bds08: NotRequired[Literal[True]]
    bds09: NotRequired[Literal[True]]
    bds0a: NotRequired[Literal[True]]
    bds20: Literal[True]
    bds40: NotRequired[Literal[True]]
    bds41: NotRequired[Literal[True]]
    bds42: NotRequired[Literal[True]]
    bds43: NotRequired[Literal[True]]
    bds44: NotRequired[Literal[True]]
    bds45: NotRequired[Literal[True]]
    bds48: NotRequired[Literal[True]]
    bds50: NotRequired[Literal[True]]
    bds51: NotRequired[Literal[True]]
    bds52: NotRequired[Literal[True]]
    bds53: NotRequired[Literal[True]]
    bds54: NotRequired[Literal[True]]
    bds55: NotRequired[Literal[True]]
    bds56: NotRequired[Literal[True]]
    bds5f: NotRequired[Literal[True]]
    bds60: NotRequired[Literal[True]]


class BDS20(TypedDict):
    bds: Literal["20"]
    callsign: str
    icao24: str


class BDS21(TypedDict):
    bds: Literal["21"]
    registration: None | str
    airline: None | str


class BDS30(TypedDict):
    bds: Literal["30"]
    issued_ra: bool
    corrective: NotRequired[bool]
    downward_sense: NotRequired[bool]
    increased_rate: NotRequired[bool]
    sense_reversal: NotRequired[bool]
    altitude_crossing: NotRequired[bool]
    positive: NotRequired[bool]
    no_below: NotRequired[bool]
    no_above: NotRequired[bool]
    no_left: NotRequired[bool]
    no_right: NotRequired[bool]
    terminated: NotRequired[bool]
    multiple: NotRequired[bool]
    threat_identity: NotRequired[str]
    threat_altitude: NotRequired[int]
    threat_range: NotRequired[float]
    threat_bearing: NotRequired[int]


class BDS40(TypedDict):
    bds: Literal["40"]
    selected_mcp: NotRequired[int]
    selected_fms: NotRequired[int]
    barometric_setting: NotRequired[float]
    target_source: Literal[
        "AircraftAltitude", "FcpMcuSelectedAltitude", "FmsSelectedAltitude"
    ]


class BDS44(TypedDict):
    bds: Literal["44"]
    issued_ra: bool
    terminated: bool
    multiple: bool
    wind_speed: None | int
    wind_direction: None | float
    temperature: float
    pressure: None | int
    turbulence: Literal[None, "Nil", "Light", "Moderate", "Severe"]
    humidity: None | float


class BDS45(TypedDict):
    bds: Literal["45"]
    turbulence: Literal[None, "Nil", "Light", "Moderate", "Severe"]
    wind_shear: Literal[None, "Nil", "Light", "Moderate", "Severe"]
    icing: Literal[None, "Nil", "Light", "Moderate", "Severe"]
    wake_vortex: Literal[None, "Nil", "Light", "Moderate", "Severe"]
    static_temperature: float
    static_pressure: int
    radio_hein: None | int


class BDS50(TypedDict):
    bds: Literal["50"]
    roll: None | float
    track: None | float
    groundspeed: None | int
    track_rate: None | float
    TAS: None | int


class BDS60(TypedDict):
    bds: Literal["60"]
    heading: NotRequired[float]
    IAS: NotRequired[int]
    Mach: NotRequired[float]
    vrate_barometric: NotRequired[int]
    vrate_inertial: NotRequired[int]


class DF0(TypedDict):
    timestamp: float
    df: Literal["0"]
    altitude: int
    icao24: str


class DF4(TypedDict):
    timestamp: float
    df: Literal["4"]
    altitude: int
    icao24: str


class DF5(TypedDict):
    timestamp: float
    df: Literal["5"]
    squawk: str
    icao24: str


class DF11(TypedDict):
    timestamp: float
    df: Literal["11"]
    capability: str
    icao24: str


class DF16(TypedDict):
    timestamp: float
    df: Literal["16"]
    vs: int
    sl: int
    ri: int
    altitude: int
    icao24: str


class DF17_BDS05(TypedDict):
    timestamp: float
    df: Literal["17"]
    icao24: str
    bds: Literal["05"]
    NUCp: int
    NICb: int
    altitude: int
    source: str
    parity: Literal["odd", "even"]
    lat_cpr: int
    lon_cpr: int
    latitude: NotRequired[float]
    longitude: NotRequired[float]


class DF17_BDS06(TypedDict):
    timestamp: float
    df: Literal["17"]
    icao24: str
    bds: Literal["06"]
    NUCp: int
    groundspeed: None | float
    track: None | float
    parity: Literal["odd", "even"]
    lat_cpr: int
    lon_cpr: int
    latitude: NotRequired[float]
    longitude: NotRequired[float]


class DF17_BDS08(TypedDict):
    timestamp: float
    df: Literal["17"]
    icao24: str
    bds: Literal["08"]
    wake_vortex: Literal[
        "n/a",
        "Surface emergency vehicle",
        "Surface service vehicle",
        "Obstruction",
        "Glider",
        "Lighter than air",
        "Parachutist",
        "Ultralight",
        "UAM",
        "Space",
        "<7000kg",
        "34,000kg",
        "<136,000kg",
        "High vortex",
        "Heavy",
        "High performance",
        "Rotorcraft",
    ]
    callsign: str


class DF17_BDS09(TypedDict):
    timestamp: float
    df: Literal["17"]
    icao24: str
    bds: Literal["09"]
    NACv: int
    groundspeed: NotRequired[float]
    TAS: NotRequired[float]
    IAS: NotRequired[float]
    track: NotRequired[float]
    heading: NotRequired[float]
    vrate_src: str
    vertical_rate: int
    geo_minus_baro: None | int


class DF17_BDS61(TypedDict):
    timestamp: float
    df: str
    icao24: str
    bds: Literal["61"]
    subtype: Literal["emergency_priority", "acas_ra"]
    emergency_state: Literal[
        "none",
        "general",
        "medical",
        "minimum_fuel",
        "no_communication",
        "unlawful_interference",
        "downed_aircraft",
        "reserved",
    ]
    squawk: str


class DF17_BDS62(TypedDict):
    timestamp: float
    df: Literal["17"]
    icao24: str
    bds: Literal["62"]
    source: str
    selected_altitude: int
    barometric_setting: float
    selected_heading: NotRequired[float]
    NACp: int
    autopilot: NotRequired[bool]
    vnav_mode: NotRequired[bool]
    alt_hold: NotRequired[bool]
    approach_mode: NotRequired[bool]
    tcas_operational: bool
    lnav_mode: NotRequired[bool]


class DF17_BDS65(TypedDict):
    timestamp: float
    df: Literal["17"]
    icao24: str
    bds: Literal["65"]
    version: Literal[0, 1, 2, 3, 4, 5, 6, 7]
    # NIC supplement A
    NICa: int
    # NIC Supplement bit (NICs)
    NICs: NotRequired[int]
    # Navigation Accuracy Category for position (NACp)
    NACp: int
    #  Geometry Vertical Accuracy (GVA)
    GVA: NotRequired[int]
    # Barometric Altitude Integrity (BAI)
    BAI: int
    # Barometric Altitude Quality (BAQ)
    BAQ: NotRequired[int]
    # Track Angle or Heading
    TAH: NotRequired[int]
    # 1 for magnetic, 0 for true north
    HRD: Literal[0, 1]
    # Surveillance Integrity Level (SIL)
    SIL: int
    SILs: int


class DF17_Unknown(TypedDict):
    timestamp: float
    df: Literal["17"]
    icao24: str
    bds: Literal["?"]


class DF18_BDS06(TypedDict):
    timestamp: float
    df: Literal["18"]
    tisb: Literal[
        "ADSB_ES_NT",
        "ADSB_ES_NT_ALT",
        "TISB_FINE",
        "TISB_COARSE",
        "TISB_MANAGE",
        "TISB_ADSB_RELAY",
        "TISB_ADSB",
        "Reserved",
    ]
    icao24: str
    bds: Literal["06"]
    NUCp: int
    groundspeed: None | float
    track: None | float
    parity: Literal["odd", "even"]
    lat_cpr: int
    lon_cpr: int
    latitude: NotRequired[float]
    longitude: NotRequired[float]


class DF18_BDS08(TypedDict):
    timestamp: float
    df: Literal["18"]
    tisb: Literal[
        "ADSB_ES_NT",
        "ADSB_ES_NT_ALT",
        "TISB_FINE",
        "TISB_COARSE",
        "TISB_MANAGE",
        "TISB_ADSB_RELAY",
        "TISB_ADSB",
        "Reserved",
    ]
    icao24: str
    bds: Literal["08"]
    wake_vortex: Literal[
        "n/a",
        "Surface emergency vehicle",
        "Surface service vehicle",
        "Obstruction",
        "Glider",
        "Lighter than air",
        "Parachutist",
        "Ultralight",
        "UAM",
        "Space",
        "<7000kg",
        "34,000kg",
        "<136,000kg",
        "High vortex",
        "Heavy",
        "High performance",
        "Rotorcraft",
    ]
    callsign: str


class DF18_BDS65(TypedDict):
    timestamp: float
    df: Literal["18"]
    tisb: Literal[
        "ADSB_ES_NT",
        "ADSB_ES_NT_ALT",
        "TISB_FINE",
        "TISB_COARSE",
        "TISB_MANAGE",
        "TISB_ADSB_RELAY",
        "TISB_ADSB",
        "Reserved",
    ]
    icao24: str
    bds: Literal["65"]
    version: Literal[0, 1, 2, 3, 4, 5, 6, 7]
    # NIC supplement A
    NICa: int
    # NIC Supplement bit (NICs)
    NICs: NotRequired[int]
    # Navigation Accuracy Category for position (NACp)
    NACp: int
    #  Geometry Vertical Accuracy (GVA)
    GVA: NotRequired[int]
    # Barometric Altitude Integrity (BAI)
    BAI: int
    # Barometric Altitude Quality (BAQ)
    BAQ: NotRequired[int]
    # Track Angle or Heading
    TAH: NotRequired[int]
    # 1 for magnetic, 0 for true north
    HRD: Literal[0, 1]
    # Surveillance Integrity Level (SIL)
    SIL: int
    SILs: int


class DF18_Unknown(TypedDict):
    timestamp: float
    df: Literal["18"]
    tisb: Literal[
        "ADSB_ES_NT",
        "ADSB_ES_NT_ALT",
        "TISB_FINE",
        "TISB_COARSE",
        "TISB_MANAGE",
        "TISB_ADSB_RELAY",
        "TISB_ADSB",
        "Reserved",
    ]
    icao24: str
    bds: Literal["?"]


class DF20(TypedDict):
    timestamp: float
    df: Literal["20"]
    altitude: int
    icao24: str
    bds05: None | BDS05
    bds10: None | BDS10
    bds17: None | BDS17
    # bds18: None | BDS18
    # bds19: None | BDS19
    bds20: None | BDS20
    bds21: None | BDS21
    bds30: None | BDS30
    bds40: None | BDS40
    bds44: None | BDS44
    bds45: None | BDS45
    bds50: None | BDS50
    bds60: None | BDS60


class DF21(TypedDict):
    timestamp: float
    df: Literal["21"]
    squawk: str
    icao24: str
    bds10: None | BDS10
    bds17: None | BDS17
    # bds18: None | BDS18
    # bds19: None | BDS19
    bds20: None | BDS20
    bds21: None | BDS21
    bds30: None | BDS30
    bds40: None | BDS40
    bds44: None | BDS44
    bds45: None | BDS45
    bds50: None | BDS50
    bds60: None | BDS60


DF17 = Union[
    DF17_Unknown,
    DF17_BDS05,
    DF17_BDS06,
    DF17_BDS08,
    DF17_BDS09,
    DF17_BDS61,
    DF17_BDS62,
    DF17_BDS65,
]

DF18 = Union[
    DF18_Unknown,
    DF18_BDS06,
    DF18_BDS08,
    DF18_BDS65,
]


Message = Union[DF0, DF4, DF5, DF11, DF16, DF17, DF18, DF20, DF21]


def is_df0(message: Message) -> TypeGuard[DF0]:
    return message["df"] == "0"


def is_df4(message: Message) -> TypeGuard[DF4]:
    return message["df"] == "4"


def is_df5(message: Message) -> TypeGuard[DF5]:
    return message["df"] == "5"


def is_df11(message: Message) -> TypeGuard[DF11]:
    return message["df"] == "11"


def is_df16(message: Message) -> TypeGuard[DF16]:
    return message["df"] == "16"


def is_df17(message: Message) -> TypeGuard[DF17]:
    return message["df"] == "17"


def is_df18(message: Message) -> TypeGuard[DF18]:
    return message["df"] == "18"


def is_df20(message: Message) -> TypeGuard[DF20]:
    return message["df"] == "20"


def is_df21(message: Message) -> TypeGuard[DF21]:
    return message["df"] == "21"


def is_bds05(message: DF17) -> TypeGuard[DF17_BDS05]:
    return message.get("bds", None) == "05" or "bds05" in message


@overload
def is_bds06(message: DF17) -> TypeGuard[DF17_BDS06]: ...
@overload
def is_bds06(message: DF18) -> TypeGuard[DF18_BDS06]: ...
def is_bds06(message: DF17 | DF18) -> TypeGuard[DF17_BDS06 | DF18_BDS06]:
    return message.get("bds", None) == "06" or "bds06" in message


@overload
def is_bds08(message: DF17) -> TypeGuard[DF17_BDS08]: ...
@overload
def is_bds08(message: DF18) -> TypeGuard[DF18_BDS08]: ...
def is_bds08(message: DF17 | DF18) -> TypeGuard[DF17_BDS08 | DF18_BDS08]:
    return message.get("bds", None) == "08" or "bds08" in message


def is_bds09(message: DF17) -> TypeGuard[DF17_BDS09]:
    return message.get("bds", None) == "09" or "bds09" in message


def is_bds10(message: DF20 | DF21) -> bool:
    return message.get("bds", None) == "10" or "bds10" in message


def is_bds17(message: DF20 | DF21) -> bool:
    return message.get("bds", None) == "17" or "bds17" in message


def is_bds20(message: DF20 | DF21) -> bool:
    return message.get("bds", None) == "20" or "bds20" in message


def is_bds30(message: DF20 | DF21) -> bool:
    return message.get("bds", None) == "30" or "bds30" in message


def is_bds40(message: DF20 | DF21) -> bool:
    return message.get("bds", None) == "40" or "bds40" in message


def is_bds44(message: DF20 | DF21) -> bool:
    return message.get("bds", None) == "44" or "bds44" in message


def is_bds50(message: DF20 | DF21) -> bool:
    return message.get("bds", None) == "50" or "bds50" in message


def is_bds60(message: DF20 | DF21) -> bool:
    return message.get("bds", None) == "60" or "bds60" in message


def is_bds61(message: DF17) -> TypeGuard[DF17_BDS61]:
    return message.get("bds", None) == "61" or "bds61" in message


def is_bds62(message: DF17) -> TypeGuard[DF17_BDS62]:
    return message.get("bds", None) == "62" or "bds62" in message


def is_bds65(message: DF17) -> TypeGuard[DF17_BDS65]:
    return message.get("bds", None) == "65" or "bds65" in message


class Flarm(TypedDict):
    timestamp: int
    reference_lat: float
    reference_lon: float
    icao24: str
    is_icao24: bool
    actype: Literal[
        "Unknown",
        "Glider",
        "Towplane",
        "Helicopter",
        "Parachute",
        "DropPlane",
        "Hangglider",
        "Paraglider",
        "Aircraft",
        "Jet",
        "UFO",
        "Balloon",
        "Airship",
        "UAV",
        "Reserved",
        "StaticObstacle",
    ]
    latitude: float
    longitude: float
    geoaltitude: int
    vertical_speed: float
    groundspeed: float
    track: float
    no_track: bool
    stealth: bool
    gps: int
