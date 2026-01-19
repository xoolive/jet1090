from pathlib import Path

import pandas as pd
import pytest

from rs1090 import flarm

root = Path(__file__)


def test_flarm() -> None:
    decoded = flarm(
        "7bf23810860b7eabb23952252fd4927024b21fd94e9e1ef416f0",
        1655274034,
        43.61924,
        5.11755,
    )
    assert decoded["icao24"] == "38f27b"
    assert decoded["is_icao24"]
    assert decoded["actype"] == "Glider"
    assert decoded["latitude"] == pytest.approx(43.61822)
    assert decoded["longitude"] == pytest.approx(5.117242)
    assert decoded["geoaltitude"] == 160
    assert decoded["vertical_speed"] == pytest.approx(-1.1)
    assert decoded["groundspeed"] == pytest.approx(0.7905694)
    assert decoded["track"] == pytest.approx(198.40446)
    assert not decoded["no_track"]
    assert not decoded["stealth"]
    assert decoded["gps"] == 3926


def test_full() -> None:
    data = pd.read_csv(
        root.parent.parent.parent / "crates/rs1090/data/flarm.csv",
    )

    decoded = flarm(
        data.rawmessage,
        data.timestamp.astype(int),
        data.sensorlatitude,
        data.sensorlongitude,
    )

    assert sum(1 for x in decoded if x["latitude"] > 47) == 0
    assert sum(1 for x in decoded if x["latitude"] < 40) == 0
    assert sum(1 for x in decoded if x["longitude"] > 12) == 0
    assert sum(1 for x in decoded if x["longitude"] < -2) == 0

    assert (
        sum(
            1
            for x in decoded
            if 43 < x["latitude"] < 44 and 4.5 < x["longitude"] < 5.5
        )
        / len(decoded)
        > 0.95
    )
